# Resolve a crate's source to a Nix store path.
#
# Takes the workspace `src` and a single crate's info from the build plan.
# Returns a path suitable for buildRustCrate's `src` argument.
#
# Local sources are filtered to exclude target/, .git/, and editor temp files.
# crates-io sources use fetchurl with SHA256 from Cargo.lock.
# Git sources use pkgs.fetchgit with a prefetched SHA256 (pure evaluation).
# Falls back to builtins.fetchGit if no SHA256 is available (requires --impure).

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

else if sourceType == "git" then
  let
    sha256 = source.sha256 or null;
    # Prefer pkgs.fetchgit with a prefetched hash (pure, fixed-output derivation).
    # Fall back to builtins.fetchGit when no hash is available (requires --impure).
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
          ("unit2nix: WARNING — git dep '${crateInfo.crateName}' has no sha256; "
            + "using builtins.fetchGit (requires --impure).\n"
            + "  To fix, run:\n"
            + "    nix-prefetch-git --url ${source.url} --rev ${source.rev}\n"
            + "  Then regenerate the build plan with `nix run .#update-plan`.")
          builtins.fetchGit {
            url = source.url;
            rev = source.rev;
          };
    subDir = source.subDir or null;
  in
  if subDir != null then repo + "/${subDir}" else repo

else
  builtins.throw ''
    unit2nix: crate ${crateInfo.crateName}-${crateInfo.version} has unknown source type "${sourceType}".
    This likely means a newer Cargo source type that unit2nix doesn't handle yet.
    Please report this at the unit2nix bug tracker with your Cargo.lock.
  ''
