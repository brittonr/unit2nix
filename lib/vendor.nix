# Vendor crate sources from Cargo.lock for sandboxed builds.
#
# Parses Cargo.lock at Nix eval time, fetches each crate source as a
# fixed-output derivation (using checksums from the lock file), and produces
# a cargo-compatible vendor directory + config.
#
# Usage:
#   let
#     vendor = import ./vendor.nix {
#       inherit pkgs;
#       cargoLock = src + "/Cargo.lock";
#       gitObjectHashesJson = src + "/git-object-hashes.json";  # for git deps
#     };
#   in {
#     inherit (vendor) vendoredSources cargoConfig;
#   }

{
  pkgs,
  lib ? pkgs.lib,
  # Path to Cargo.lock
  cargoLock,
  # Optional: hash map for canonical, revision-scoped Git objects.
  gitObjectHashesJson ? null,
}:

let
  locked = lib.importTOML cargoLock;

  gitObjectHashes =
    if gitObjectHashesJson != null && builtins.pathExists gitObjectHashesJson
    then builtins.fromJSON (builtins.readFile gitObjectHashesJson)
    else { };

  # Classify packages by source type.
  # Local packages (no source field) are skipped — they come from workspace src.
  packages =
    let
      all = locked.package or [ ];
      withSource = builtins.filter (p: p ? source) all;
      # Deduplicate by "name version (source)"
      byId = builtins.listToAttrs (
        map (p: { name = "${p.name} ${p.version} (${p.source})"; value = p; }) withSource
      );
    in
    builtins.attrValues byId;

  sourceType = pkg:
    if lib.hasPrefix "registry+" pkg.source then "crates-io"
    else if lib.hasPrefix "git+" pkg.source then "git"
    else null;

  packagesByType = lib.groupBy (pkg: sourceType pkg) (
    builtins.filter (pkg: sourceType pkg != null) packages
  );

  # --- crates.io fetching ---

  # Unpack a .crate tarball and add .cargo-checksum.json
  unpackCrate = { name, version, checksum, ... }:
    let
      src = pkgs.fetchurl {
        name = "${name}-${version}.tar.gz";
        url = "https://static.crates.io/crates/${name}/${name}-${version}.crate";
        sha256 = checksum;
      };
    in
    pkgs.runCommand "${name}-${version}" { } ''
      mkdir -p $out
      tar -xzf ${src} --strip-components=1 -C $out
      echo '{"package":"${checksum}","files":{}}' > $out/.cargo-checksum.json
    '';

  cratesIoSources = map (pkg: {
    name = "${pkg.name}-${pkg.version}";
    path = unpackCrate pkg;
  }) (packagesByType."crates-io" or [ ]);

  # --- git source handling ---
  #
  # Git deps are NOT vendored into the linkFarm directory (cargo's directory
  # vendor format can't handle workspace inheritance like `rust-version.workspace = true`).
  # Instead, we fetch whole repos and expose them for CARGO_HOME/git/ cache population.

  parseGitSource = source:
    let
      withoutGitPlus = lib.removePrefix "git+" source;
      splitHash = lib.splitString "#" withoutGitPlus;
      preFragment = builtins.elemAt splitHash 0;
      fragment =
        if builtins.length splitHash >= 2
        then builtins.elemAt splitHash 1
        else null;
      splitQuestion = lib.splitString "?" preFragment;
      url = builtins.elemAt splitQuestion 0;
      # Extract only the ?rev= query param (the only one we consume).
      queryString =
        if builtins.length splitQuestion >= 2
        then builtins.elemAt splitQuestion 1
        else "";
      queryParts = lib.splitString "&" queryString;
      revParam = builtins.filter (s: lib.hasPrefix "rev=" s) queryParts;
      rev =
        if revParam != []
        then lib.removePrefix "rev=" (builtins.head revParam)
        else null;
    in
    { inherit url fragment rev; };

  # Hash key format matching crate2nix convention
  toHashKey = pkg:
    let sourceBase = builtins.head (lib.splitString "#" (lib.removePrefix "git+" pkg.source));
    in "${sourceBase}#${pkg.name}@${pkg.version}";

  toPackageId = pkg: "${pkg.name} ${pkg.version} (${pkg.source})";

  # Group git packages by repo (same URL + rev), fetch each repo once.
  gitRepoKey = pkg:
    let parsed = parseGitSource pkg.source;
    in "${parsed.url}#${
      if parsed.fragment != null then parsed.fragment
      else parsed.rev or ""
    }";

  gitRepoPkgs = packagesByType."git" or [ ];
  gitRepoGroups = lib.groupBy gitRepoKey gitRepoPkgs;

  fetchGitRepo = group:
    let
      representativePkg = builtins.head group;
      parsed = parseGitSource representativePkg.source;
      hashKey = toHashKey representativePkg;
      hashes = lib.unique (builtins.filter (hash: hash != null) (map (pkg:
        gitObjectHashes.${toHashKey pkg} or gitObjectHashes.${toPackageId pkg} or null
      ) group));
      sha256 =
        if hashes == [] then null
        else if builtins.length hashes == 1 then builtins.head hashes
        else builtins.throw "unit2nix: conflicting Git-object hashes for ${parsed.url} at ${rev}";

      rev =
        if parsed.fragment != null then parsed.fragment
        else parsed.rev or (builtins.throw "unit2nix: git dep '${representativePkg.name}' has no rev in source URL");

      # Keep the source-tree hash in crate-hashes.json separate from this
      # revision-scoped object database. fetchgit's raw .git directory contains
      # mutable refs, logs and pack files: hashing it directly is not stable.
      src =
        if sha256 != null then
          pkgs.fetchgit {
            inherit sha256;
            inherit (parsed) url;
            inherit rev;
            fetchSubmodules = true;
            leaveDotGit = true;
            postFetch = ''
              git init --bare -q "$out/.git-canonical"
              rm -rf "$out/.git-canonical/hooks"
              git -C "$out" rev-list --objects --no-walk ${lib.escapeShellArg rev} \
                | git -C "$out" pack-objects --stdout \
                | git --git-dir="$out/.git-canonical" unpack-objects
              printf '%s\n' ${lib.escapeShellArg rev} > "$out/.git-canonical/shallow"
              git --git-dir="$out/.git-canonical" update-ref refs/heads/_cargo_head ${lib.escapeShellArg rev}
              echo 'ref: refs/heads/_cargo_head' > "$out/.git-canonical/HEAD"
              rm -rf "$out/.git"
              mv "$out/.git-canonical" "$out/.git"
              find "$out" -mindepth 1 -maxdepth 1 ! -name .git -exec rm -rf {} +
            '';
          }
        else
          builtins.throw ''
            unit2nix: git dependency "${representativePkg.name}" from ${parsed.url} at ${rev}
            requires a canonical Git-object SHA256 in git-object-hashes.json.
            This hash is distinct from the source-tree hash in crate-hashes.json.
            Fetch the exact revision with fetchgit leaveDotGit=true and the
            canonical postFetch in lib/vendor.nix, then record its resulting hash
            under "${hashKey}" in git-object-hashes.json.
          '';
    in
    {
      inherit rev src;
      inherit (parsed) url;
    };

  # Fetched git repos: { "url#rev" = { src, url, rev }; }
  gitRepos = lib.mapAttrs (_: group: fetchGitRepo group) gitRepoGroups;

  # Git repos are NOT put in the vendor linkFarm. Instead, auto.nix populates
  # CARGO_HOME/git/checkouts/ so cargo finds them without network access.
  # We export the repo list for auto.nix to consume.
  gitCheckouts = lib.mapAttrsToList (_: repo: repo) gitRepos;

  # --- vendor directory + config ---

  vendoredSources = pkgs.linkFarm "cargo-vendor" cratesIoSources;

  # Generate cargo config.
  # crates-io deps are redirected to the vendor directory.
  # Git deps are NOT redirected — they're handled via CARGO_HOME/git/ cache.
  cargoConfig = pkgs.writeText "cargo-vendor-config" ''
    [source.crates-io]
    replace-with = "vendored-sources"

    [source.vendored-sources]
    directory = "${vendoredSources}"
  '';

in {
  inherit vendoredSources cargoConfig gitCheckouts;
}
