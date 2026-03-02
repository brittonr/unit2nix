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

  # --- git source fetching ---

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

  fetchGitDep = pkg:
    let
      parsed = parseGitSource pkg.source;
      hashKey = toHashKey pkg;
      packageId = toPackageId pkg;
      sha256 = crateHashes.${hashKey} or crateHashes.${packageId} or null;

      rev =
        if parsed.fragment != null then parsed.fragment
        else parsed.rev or (builtins.throw "git dep ${pkg.name} has no rev");

      repo =
        if sha256 != null then
          pkgs.fetchgit {
            inherit sha256;
            inherit (parsed) url;
            inherit rev;
            fetchSubmodules = true;
          }
        else
          builtins.fetchGit {
            inherit (parsed) url;
            inherit rev;
            submodules = true;
          };

      # Find the right subdirectory for this crate within the git repo
      allCargoTomls = lib.filter
        (lib.hasSuffix "Cargo.toml")
        (lib.filesystem.listFilesRecursive repo);

      getCrateName = path:
        let toml = builtins.fromTOML (builtins.readFile path);
        in toml.package.name or null;

      matchingToml = builtins.head (
        builtins.filter (p: getCrateName p == pkg.name) allCargoTomls
      );

      crateDir = lib.removeSuffix "Cargo.toml" matchingToml;
    in
    pkgs.runCommand "${pkg.name}-${pkg.version}" { } ''
      mkdir -p $out
      cp -a ${crateDir}* $out/ 2>/dev/null || cp -a ${crateDir}. $out/
      echo '{"package":null,"files":{}}' > $out/.cargo-checksum.json
    '';

  gitSources = map (pkg: {
    name = "${pkg.name}-${pkg.version}";
    path = fetchGitDep pkg;
  }) (packagesByType."git" or [ ]);

  # --- vendor directory + config ---

  vendoredSources = pkgs.linkFarm "cargo-vendor" (cratesIoSources ++ gitSources);

  # Generate cargo config that redirects all sources to vendored dir
  gitSourceConfigs =
    let
      gitPkgs = packagesByType."git" or [ ];
      uniqueSources = lib.unique (map (p: p.source) gitPkgs);
      mkConfig = source:
        let parsed = parseGitSource source;
        in ''

          [source."${lib.removePrefix "git+" source}"]
          git = "${parsed.url}"
          ${lib.optionalString (parsed ? rev) ''rev = "${parsed.rev}"''}
          ${lib.optionalString (parsed ? tag) ''tag = "${parsed.tag}"''}
          ${lib.optionalString (parsed ? branch) ''branch = "${parsed.branch}"''}
          replace-with = "vendored-sources"
        '';
    in
    lib.concatMapStrings mkConfig uniqueSources;

  cargoConfig = pkgs.writeText "cargo-vendor-config" ''
    [source.crates-io]
    replace-with = "vendored-sources"
    ${gitSourceConfigs}
    [source.vendored-sources]
    directory = "${vendoredSources}"
  '';

in {
  inherit vendoredSources cargoConfig;
}
