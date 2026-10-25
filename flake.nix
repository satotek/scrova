{
  description = "Scrova development environment";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    { nixpkgs, rust-overlay, ... }:
    let
      systems = [
        "aarch64-darwin"
        "x86_64-darwin"
        "aarch64-linux"
        "x86_64-linux"
      ];
      forAllSystems =
        f:
        nixpkgs.lib.genAttrs systems (
          system:
          f (
            import nixpkgs {
              inherit system;
              overlays = [ rust-overlay.overlays.default ];
            }
          )
        );
    in
    {
      devShells = forAllSystems (pkgs: {
        # mkShellNoCC: Nix の clang / SDKROOT を注入すると iOS ターゲットのリンクで
        # Xcode の SDK と食い違うため、C ツールチェーンは Xcode (xcrun) に任せる。
        default = pkgs.mkShellNoCC {
          # nix develop still exports the stdenv's LD=ld. xcodebuild then links
          # Swift packages with that wrapper, which rejects Apple's -isysroot.
          shellHook = ''
            unset LD LD_FOR_BUILD
            unset NIX_LDFLAGS NIX_LDFLAGS_FOR_BUILD
            unset NIX_CFLAGS_COMPILE NIX_CFLAGS_COMPILE_FOR_BUILD
          '';
          packages = [
            (pkgs.rust-bin.stable.latest.default.override {
              extensions = [
                "rust-src"
                "rust-analyzer"
                "clippy"
                "rustfmt"
              ];
              targets = pkgs.lib.optionals pkgs.stdenv.hostPlatform.isDarwin [
                "aarch64-apple-darwin"
                "x86_64-apple-darwin"
                "aarch64-apple-ios"
                "aarch64-apple-ios-sim"
              ];
            })
            pkgs.sqlite
          ]
          ++ pkgs.lib.optionals pkgs.stdenv.hostPlatform.isDarwin [
            pkgs.xcodegen
          ];
        };
      });
    };
}
