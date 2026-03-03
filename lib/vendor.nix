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
#       crateHashesJson = src + "/crate-hashes.json";  # optional, for git deps
#     };
#   in {
#     inherit (vendor) vendoredSources cargoConfig;
#   }

{
  pkgs,
  lib ? pkgs.lib,
  # Path to Cargo.lock
  cargoLock,
  # Optional: path to crate-hashes.json (SHA256 hashes for git deps)
  crateHashesJson ? null,
}:

let
  locked = lib.importTOML cargoLock;

  crateHashes =
    if crateHashesJson != null && builtins.pathExists crateHashesJson
    then builtins.fromJSON (builtins.readFile crateHashesJson)
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
      queryParamsList = lib.optionals
        (builtins.length splitQuestion >= 2)
        (lib.splitString "&" (builtins.elemAt splitQuestion 1));
      kv = s:
        let parts = lib.splitString "=" s;
        in {
          name =
            let key = builtins.elemAt parts 0;
            in if key == "ref" then "branch" else key;
          value = builtins.elemAt parts 1;
        };
      queryParams = builtins.listToAttrs (map kv queryParamsList);
    in
    queryParams // { inherit url fragment; };

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

  fetchGitRepo = representativePkg:
    let
      parsed = parseGitSource representativePkg.source;
      hashKey = toHashKey representativePkg;
      packageId = toPackageId representativePkg;
      sha256 = crateHashes.${hashKey} or crateHashes.${packageId} or null;

      rev =
        if parsed.fragment != null then parsed.fragment
        else parsed.rev or (builtins.throw "unit2nix: git dep '${representativePkg.name}' has no rev in source URL");

      # For the auto-build git wrapper, we need actual git repos (with .git).
      # fetchgit with leaveDotGit preserves it. Without a known sha256, we
      # can't use fetchgit, so git deps without hashes in crate-hashes.json
      # will cause a build failure with a clear error message.
      src =
        if sha256 != null then
          pkgs.fetchgit {
            inherit sha256;
            inherit (parsed) url;
            inherit rev;
            fetchSubmodules = true;
            leaveDotGit = true;
          }
        else
          builtins.throw ''
            unit2nix: git dependency "${representativePkg.name}" from ${parsed.url} at ${rev}
            requires a SHA256 hash in crate-hashes.json for auto mode.

            Step 1 — get the hash:
              nix-prefetch-git --url ${parsed.url} --rev ${rev} --fetch-submodules | jq -r .sha256

            Step 2 — add it to crate-hashes.json in your workspace root:
              {
                "${toHashKey representativePkg}": "<sha256 from step 1>"
              }

            Step 3 — rebuild. The hash is cached; this is a one-time step per git rev.
          '';
    in
    {
      inherit rev src;
      inherit (parsed) url;
    };

  # Fetched git repos: { "url#rev" = { src, url, rev }; }
  gitRepos = lib.mapAttrs (_: group: fetchGitRepo (builtins.head group)) gitRepoGroups;

  # Git repos are NOT put in the vendor linkFarm. Instead, auto.nix populates
  # CARGO_HOME/git/checkouts/ so cargo finds them without network access.
  # We export the repo list for auto.nix to consume.
  gitCheckouts = lib.mapAttrsToList (_: repo: repo) gitRepos;

  gitSources = [];

  # --- vendor directory + config ---

  vendoredSources = pkgs.linkFarm "cargo-vendor" (cratesIoSources ++ gitSources);

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
