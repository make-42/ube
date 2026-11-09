{
  description = "Rust application with a NixOS development shell (including libclang for bindgen)";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };

        # Keep the compiler and bindgen's libclang from the same nixpkgs set.
        rustToolchain = pkgs.rustc;

        app = pkgs.rustPlatform.buildRustPackage {
          pname = "ube";
          version = "0.1.0";
          src = ./.;

          # This expects Cargo.lock to be present in the project root.
          cargoLock.lockFile = ./Cargo.lock;

          nativeBuildInputs = with pkgs; [
            pkg-config
            cmake
            clang
            llvmPackages.libclang
          ];

          buildInputs = with pkgs; [
            llvmPackages.libclang
          ];

          # bindgen uses libclang at build time. This fixes the
          # "Unable to find libclang" panic on NixOS.
          LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";
          BINDGEN_EXTRA_CLANG_ARGS = "-I${pkgs.glibc.dev}/include";

          # Remove this if you want Cargo tests to run during nix build.
          doCheck = false;
        };
      in
      {
        packages.default = app;

        devShells.default = pkgs.mkShell {
          packages = with pkgs; [
            rustToolchain
            cargo
            rustfmt
            clippy
            rust-analyzer
            pkg-config
            cmake
            clang
            llvmPackages.libclang
          ];

          LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";
          BINDGEN_EXTRA_CLANG_ARGS = "-I${pkgs.glibc.dev}/include";

          shellHook = ''
            echo "Rust development shell (libclang available for bindgen)"
          '';
        };
      });
}
