{
  description = "iwaya";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  };

  outputs = { self, nixpkgs }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];

      forAllSystems = nixpkgs.lib.genAttrs systems;

      cargoToml =
        builtins.fromTOML (builtins.readFile ./Cargo.toml);
    in
    {
      packages = forAllSystems (system:
        let
          pkgs = import nixpkgs { inherit system; };
        in
        {
          default = pkgs.rustPlatform.buildRustPackage {
            pname = cargoToml.package.name;
            version = cargoToml.package.version;

            src = pkgs.lib.cleanSource ./.;

            cargoLock.lockFile = ./Cargo.lock;

            nativeBuildInputs = [
              pkgs.makeWrapper
            ];

            postInstall = ''
              wrapProgram "$out/bin/iwaya" \
                --suffix PATH : ${pkgs.lib.makeBinPath [ pkgs.bws ]}
            '';

            meta = {
              description = "Command proxy for controlled secret injection";
              license = pkgs.lib.licenses.asl20;
              mainProgram = "iwaya";
            };
          };
        });
    };
}

