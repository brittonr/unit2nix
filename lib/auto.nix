# Auto-build: generate build-plan.json via IFD and build with buildFromUnitGraph.
#
# Vendors crate sources from Cargo.lock, runs unit2nix in a sandboxed
# derivation, and imports the result at eval time (IFD). No manual
# regeneration step needed.
#
# Requires: IFD enabled (default in Nix; disabled on Hydra).
#
# Usage:
#   let
#     ws = import ./auto.nix {
#       inherit pkgs;
#       unit2nix = <unit2nix package>;
#       src = ./.;
#     };
#   in ws.workspaceMembers.my-crate.build

{
  pkgs,
  lib ? pkgs.lib,
  # The unit2nix Nix package (with cargo/rustc on PATH)
  unit2nix,
  # Workspace source root
  src,
  # Optional: subdirectory within src containing Cargo.toml.
  # Use when src is a parent directory containing multiple repos
  # (e.g., for workspaces with `path = "../sibling"` dependencies).
  workspaceDir ? null,
  # Optional: buildRustCrate override (forwarded to buildFromUnitGraph)
  buildRustCrateForPkgs ? pkgs: pkgs.buildRustCrate,
  # Optional: full override of the crate overrides base layer (forwarded to buildFromUnitGraph).
  # When provided, replaces both pkgs.defaultCrateOverrides and unit2nix built-ins.
  defaultCrateOverrides ? null,
  # Optional: additional crate overrides on top of defaults (forwarded to buildFromUnitGraph)
  extraCrateOverrides ? {},
  # Optional: extra arguments passed to clippy-driver (e.g. ["-D" "warnings"])
  clippyArgs ? [],
  # Optional: filter which workspace members are exposed (forwarded to buildFromUnitGraph)
  members ? null,
  # Optional: Rust toolchain (e.g. nightly from rust-overlay) for the IFD step.
  # The unit2nix wrapper bundles stable cargo/rustc, but `cargo --unit-graph`
  # requires nightly. When set, this toolchain is prepended to PATH inside the
  # IFD derivation, overriding the wrapper's stable cargo/rustc.
  rustToolchain ? null,
  # Pass --workspace to cargo for per-crate test support.
  # When true, ALL workspace members are resolved (including dev-deps),
  # enabling test.check.<member> for every crate in the workspace.
  workspace ? false,
  # Optional: build a specific package (passed as -p to unit2nix)
  package ? null,
  # Optional: features to enable (comma-separated string, passed as --features)
  features ? null,
  # Optional: enable all features
  allFeatures ? false,
  # Optional: disable default features
  noDefaultFeatures ? false,
  # Optional: include dev-dependencies in the resolve
  includeDev ? false,
}:

let
  # The actual workspace root (for Cargo.toml, Cargo.lock, etc.)
  workspaceSrc = if workspaceDir != null then src + "/${workspaceDir}" else src;

  cargoLockPath = workspaceSrc + "/Cargo.lock";
  crateHashesPath = workspaceSrc + "/crate-hashes.json";

  manifestRelPath =
    if workspaceDir != null
    then "${workspaceDir}/Cargo.toml"
    else "Cargo.toml";

  vendor = import ./vendor.nix {
    inherit pkgs lib;
    cargoLock = cargoLockPath;
    crateHashesJson =
      if builtins.pathExists crateHashesPath
      then crateHashesPath
      else null;
  };

  # Map of git URL → pre-fetched store path for the git wrapper.
  # Format: one "url rev storepath" per line.
  gitRepoMap = lib.concatMapStrings (repo: ''
    ${repo.url} ${repo.rev} ${repo.src}
  '') vendor.gitCheckouts;

  gitRepoMapFile = pkgs.writeText "git-repo-map" gitRepoMap;

  # Wrapper script that intercepts `git` clone/fetch operations.
  # Cargo with net.git-fetch-with-cli invokes:
  #   git clone --bare <url> <dest>       (first time)
  #   git fetch <url> <refspec>           (updates / rev resolution)
  # We intercept both, serving from pre-fetched nix store paths.
  fakeGit = pkgs.writeShellScript "fake-git" ''
    REAL_GIT="${pkgs.git}/bin/git"

    # Extract the remote URL from args (first non-flag, non-command positional)
    find_url() {
      local skip_next=""
      for arg in "$@"; do
        if [ -n "$skip_next" ]; then skip_next=""; continue; fi
        case "$arg" in
          clone|fetch|--bare|--no-tags|--force|--update-head-ok|-q|--quiet) ;;
          --config|--upload-pack|-o|-t|-j|--depth|--shallow-since) skip_next=1 ;;
          --config=*|--upload-pack=*|-o=*) ;;
          -*)  ;;
          *://*) echo "$arg"; return ;;
          *) ;;
        esac
      done
    }

    url="$(find_url "$@")"

    # Check if this URL matches a pre-fetched repo
    local_path=""
    local_rev=""
    if [ -n "$url" ]; then
      while IFS=' ' read -r map_url map_rev map_path; do
        if [ "$map_url" = "$url" ]; then
          local_path="$map_path"
          local_rev="$map_rev"
          break
        fi
      done < ${gitRepoMapFile}
    fi

    if [ -z "$local_path" ]; then
      # Not a pre-fetched repo — pass through to real git
      exec "$REAL_GIT" "$@"
    fi

    case "$1" in
      clone)
        # git clone --bare <url> <dest>
        dest="''${@: -1}"
        "$REAL_GIT" init --bare "$dest" 2>/dev/null
        "$REAL_GIT" -C "$dest" fetch "$local_path" '+HEAD:refs/heads/_cargo_head' 2>/dev/null || true
        # Import all objects so cargo can resolve any rev
        "$REAL_GIT" -C "$dest" fetch "$local_path" 2>/dev/null || \
        "$REAL_GIT" -C "$dest" fetch "$local_path/.git" 2>/dev/null || true
        exit 0
        ;;
      fetch)
        # git fetch [opts] <url> <refspec...>
        # Replace the remote URL with the local path, keep everything else
        args=()
        replaced=false
        for arg in "$@"; do
          if [ "$arg" = "$url" ] && [ "$replaced" = false ]; then
            args+=("$local_path")
            replaced=true
          else
            args+=("$arg")
          fi
        done
        exec "$REAL_GIT" "''${args[@]}"
        ;;
      *)
        exec "$REAL_GIT" "$@"
        ;;
    esac
  '';

  # Generate build-plan.json in a sandboxed derivation.
  # Cargo uses vendored sources (no network access needed).
  hasGitDeps = vendor.gitCheckouts != [];

  generatedPlan = pkgs.runCommand "unit2nix-build-plan" {
    nativeBuildInputs = [ unit2nix ] ++ lib.optional hasGitDeps pkgs.git;
    preferLocalBuild = true;
  } ''
    ${lib.optionalString (rustToolchain != null) ''
      # Prepend user-supplied toolchain (e.g. nightly) so it overrides the
      # stable cargo/rustc bundled in the unit2nix wrapper.
      export PATH="${rustToolchain}/bin:$PATH"
    ''}

    # Set up vendored cargo home
    export CARGO_HOME=$(mktemp -d)
    mkdir -p "$CARGO_HOME"

    # Cargo config: vendor crates-io deps, use CLI git for git deps
    cat ${vendor.cargoConfig} > "$CARGO_HOME/config.toml"
    ${lib.optionalString hasGitDeps ''
      cat >> "$CARGO_HOME/config.toml" <<'GITCFG'

      [net]
      git-fetch-with-cli = true
    GITCFG

      # Put our git wrapper first on PATH so cargo uses it
      mkdir -p /tmp/fake-git-bin
      cp ${fakeGit} /tmp/fake-git-bin/git
      chmod +x /tmp/fake-git-bin/git
      export PATH="/tmp/fake-git-bin:$PATH"
    ''}

    # Copy source tree (preserving directory structure for relative path deps)
    cp -r ${src} source
    chmod -R u+w source
    cd source

    unit2nix --manifest-path ./${manifestRelPath} -o "$out" --no-check \
      ${lib.optionalString workspace "--workspace"} \
      ${lib.optionalString (package != null) "-p ${lib.escapeShellArg package}"} \
      ${lib.optionalString (features != null) "--features ${lib.escapeShellArg features}"} \
      ${lib.optionalString allFeatures "--all-features"} \
      ${lib.optionalString noDefaultFeatures "--no-default-features"} \
      ${lib.optionalString includeDev "--include-dev"} \
      ${lib.optionalString (members != null) "--members ${lib.escapeShellArg (builtins.concatStringsSep "," members)}"}
  '';

in
import ./build-from-unit-graph.nix {
  inherit pkgs lib buildRustCrateForPkgs defaultCrateOverrides extraCrateOverrides clippyArgs members;
  src = workspaceSrc;
  resolvedJson = generatedPlan;
  skipStalenessCheck = true;
}
