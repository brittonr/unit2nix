# Resolve a crate's source to a Nix store path.
#
# Takes the workspace `src` and a single crate's info from the build plan.
# Returns a path suitable for buildRustCrate's `src` argument.

{ pkgs, src }:

crateInfo:
let
  source = crateInfo.source or null;
  sourceType = if source == null then "local" else source.type or "local";
in
if sourceType == "local" then
  let
    relPath = if source == null then "." else source.path or ".";
  in
  if relPath == "." then src else src + "/${relPath}"

else if sourceType == "crates-io" then
  pkgs.fetchurl {
    name = "${crateInfo.crateName}-${crateInfo.version}.tar.gz";
    url = "https://static.crates.io/crates/${crateInfo.crateName}/${crateInfo.crateName}-${crateInfo.version}.crate";
    sha256 = crateInfo.sha256;
  }

else if sourceType == "git" then
  builtins.fetchGit {
    url = source.url;
    rev = source.rev;
  }

else
  # Fallback: treat as local
  src
