# Resolve a crate's source to a Nix store path.
#
# Takes the workspace `src` and a single crate's info from the build plan.
# Returns a path suitable for buildRustCrate's `src` argument.
#
# Local sources are filtered to exclude target/, .git/, and editor temp files.
# crates-io sources use fetchurl with SHA256 from Cargo.lock.
# Git sources use pkgs.fetchgit with a prefetched SHA256 (pure evaluation).
# Stdlib sources resolve from rustSrcPath (the toolchain's library source).
# Falls back to builtins.fetchGit if no SHA256 is available (requires --impure).

{
  pkgs,
  src,
  # Path to the Rust toolchain's stdlib source (e.g. "${rustToolchain}/lib/rustlib/src/rust").
  # Required when the build plan contains stdlib crates (from --build-std).
  rustSrcPath ? null,
  # Map of absolute filesystem paths to Nix store paths for out-of-tree path deps.
  # See build-from-unit-graph.nix externalSources for documentation.
  externalSources ? {},
  # Optional extra filter for local crate sources.
  # Receives the same (path: type:) arguments as cleanSourceWith.filter.
  localSourceFilter ? null,
}:

crateInfo:
let
  source = crateInfo.source or null;
  sourceType = if source == null then "local" else source.type or "local";
in
if sourceType == "local" || sourceType == null then
  let
    relPath = if source == null then "." else source.path or ".";
    isAbsolute = builtins.substring 0 1 relPath == "/";
    isNixStorePath = pkgs.lib.hasPrefix "/nix/store/" relPath;
    hasExternalOverride = isAbsolute && externalSources ? ${relPath};
    rawSrc =
      if relPath == "." then
        src
      else if hasExternalOverride then
        externalSources.${relPath}
      # Nix store paths are already available in the sandbox — use directly.
      # This happens when auto.nix symlinks external sources into the IFD
      # build dir and cargo canonicalizes through them to store paths.
      else if isNixStorePath then
        /. + relPath
      else if isAbsolute then
        builtins.throw ''
          unit2nix: local path dependency has absolute path outside the workspace:
            ${relPath}

          This path is not available inside the Nix sandbox. Fix options:

          1. Regenerate the build plan — the CLI now auto-resolves out-of-tree
             path deps as git sources when the directory is a git repo:
               nix run .#update-plan

          2. Provide the source via externalSources in your flake.nix:
               ws = import "''${unit2nix}/lib/build-from-unit-graph.nix" {
                 inherit pkgs;
                 src = ./.;
                 resolvedJson = ./build-plan.json;
                 externalSources."${relPath}" = some-flake-input + "/subdir";
               };

          3. Convert to a git dependency in Cargo.toml:
               [dependencies]
               ${crateInfo.crateName} = { git = "https://...", rev = "..." }
        ''
      else
        src + "/${relPath}";
  in
  pkgs.lib.cleanSourceWith {
    src = rawSrc;
    filter =
      path: type:
      let
        baseName = builtins.baseNameOf path;
        keepByDefault =
          # Standard VCS/editor filtering
          (pkgs.lib.cleanSourceFilter path type)
          # Cargo build artifacts
          && baseName != "target"
          # Common local noise that should not perturb crate store paths.
          && baseName != ".direnv"
          && baseName != "result"
          && !(pkgs.lib.hasPrefix "result-" baseName);
        keepByCaller = if localSourceFilter == null then true else localSourceFilter path type;
      in
      keepByDefault && keepByCaller;
  }

else if sourceType == "crates-io" then
  pkgs.fetchurl {
    name = "${crateInfo.crateName}-${crateInfo.version}.tar.gz";
    url = "https://static.crates.io/crates/${crateInfo.crateName}/${crateInfo.crateName}-${crateInfo.version}.crate";
    sha256 = crateInfo.sha256;
  }

else if sourceType == "registry" then
  # Alternative registry — download URL must be provided via crate overrides
  # since there's no standard download URL convention across registries.
  # The source.index field contains the registry index URL for reference.
  builtins.throw ''
    unit2nix: crate ${crateInfo.crateName}-${crateInfo.version} uses alternative registry: ${source.index or "unknown"}
    Alternative registries are not yet auto-fetched. Override the source via buildRustCrateForPkgs:
      buildRustCrateForPkgs = pkgs: (pkgs.buildRustCrate.override {
        defaultCrateOverrides = pkgs.defaultCrateOverrides // {
          ${crateInfo.crateName} = attrs: {
            src = pkgs.fetchurl { url = "..."; sha256 = "..."; };
          };
        };
      });
    Registry index: ${source.index or "unknown"}
  ''

else if sourceType == "stdlib" then
  let
    relPath = source.path or (builtins.throw "stdlib source missing 'path' field for ${crateInfo.crateName}");
  in
  if rustSrcPath == null then
    builtins.throw ''
      unit2nix: crate ${crateInfo.crateName}-${crateInfo.version} is a stdlib crate (from -Z build-std)
      but no rustSrcPath was provided. Pass rustSrcPath to buildFromUnitGraph:
        buildFromUnitGraph {
          rustSrcPath = "''${rustToolchain}/lib/rustlib/src/rust";
          ...
        };
    ''
  else
    rustSrcPath + "/${relPath}"

else if sourceType == "git" then
  let
    sha256 = source.sha256 or null;
    # Prefer fixed-output fetches when a hash is available.
    # This makes git sources first-class substitutable store objects instead of
    # eval-time builtins.fetchGit results.
    repo =
      if sha256 != null then
        pkgs.fetchgit {
          url = source.url;
          rev = source.rev;
          inherit sha256;
          fetchSubmodules = true;
        }
      else
        builtins.trace
          "unit2nix: WARNING — git source ${source.url} at ${source.rev} has no sha256; using builtins.fetchGit reduces cross-machine cacheability"
          (builtins.fetchGit {
            url = source.url;
            rev = source.rev;
            allRefs = true;
            submodules = true;
          });
    subDir = source.subDir or null;
  in
  if subDir != null then repo + "/${subDir}" else repo

else
  builtins.throw ''
    unit2nix: crate ${crateInfo.crateName}-${crateInfo.version} has unknown source type "${sourceType}".
    This likely means a newer Cargo source type that unit2nix doesn't handle yet.
    Please report this at the unit2nix bug tracker with your Cargo.lock.
  ''
