{
  description = "Frontend for configuring/building/testing CMake projects";

  inputs = {
    nixpkgs.url = "https://channels.nixos.org/nixpkgs-unstable/nixexprs.tar.xz";
    flake-parts.url = "github:hercules-ci/flake-parts";
  };

  outputs =
    inputs@{
      flake-parts,
      ...
    }:
    flake-parts.lib.mkFlake { inherit inputs; } {
      systems = inputs.nixpkgs.lib.systems.flakeExposed;
      perSystem =
        { pkgs, lib, ... }:
        let
          toml = (lib.importTOML ./Cargo.toml).package;
        in
        {
          packages = rec {
            cm = pkgs.rustPlatform.buildRustPackage (finalAttrs: {
              pname = toml.name;
              inherit (toml) version;
              cargoLock = {
                lockFile = ./Cargo.lock;
                allowBuiltinFetchGit = true;
              };
              src = ./.;
              nativeBuildInputs = [ pkgs.installShellFiles ];
              postInstall = ''
                installShellCompletion --cmd cm \
                  --bash gen/cm.bash \
                  --fish gen/cm.fish \
                  --zsh gen/_cm
                installManPage gen/*.1
              '';
            });
            default = cm;
          };
          formatter = pkgs.nixfmt-tree;
        };
    };
}
