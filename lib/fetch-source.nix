# Resolve a crate's source to a Nix store path.
#
# Takes the workspace `src` and a single crate's info from the build plan.
# Returns a path suitable for buildRustCrate's `src` argument.
#
# Local sources are filtered to exclude target/, .git/, and editor temp files.
# crates-io sources use fetchurl with SHA256 from Cargo.lock.
# Git sources use builtins.fetchGit with a pinned rev.

{ pkgs, src }:

crateInfo:
let
  source = crateInfo.source or null;
  sourceType = if source == null then "local" else source.type or "local";
in
if sourceType == "local" || sourceType == null then
  let
    relPath = if source == null then "." else source.path or ".";
    rawSrc = if relPath == "." then src else src + "/${relPath}";
  in
  pkgs.lib.cleanSourceWith {
    src = rawSrc;
    filter =
      path: type:
      let
        baseName = builtins.baseNameOf path;
      in
      # Standard VCS/editor filtering
      (pkgs.lib.cleanSourceFilter path type)
      # Cargo build artifacts
      && baseName != "target";
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
    Crate ${crateInfo.crateName}-${crateInfo.version} uses alternative registry: ${source.index or "unknown"}
    Alternative registries are not yet auto-fetched. Provide the source via defaultCrateOverrides:
      defaultCrateOverrides = pkgs.defaultCrateOverrides // {
        ${crateInfo.crateName} = attrs: { src = fetchurl { ... }; };
      };
  ''

else if sourceType == "git" then
  let
    repo = builtins.fetchGit {
      url = source.url;
      rev = source.rev;
    };
    subDir = source.subDir or null;
  in
  if subDir != null then repo + "/${subDir}" else repo

else
  # Fallback: treat as local
  src
