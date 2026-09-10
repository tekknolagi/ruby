{
  description = "CRuby with ZJIT, built by Nix so any revision can boot in the browser via trynix.dev";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }:
    let
      inherit (nixpkgs) lib;
      systems = [ "x86_64-linux" "aarch64-linux" "aarch64-darwin" ];
      forAllSystems = f: lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
    in
    {
      packages = forAllSystems (pkgs: rec {
        default = ruby-zjit;
        ruby-zjit = pkgs.stdenv.mkDerivation {
          pname = "ruby-zjit";
          version = "dev-${self.shortRev or self.dirtyShortRev or "dirty"}";
          src = self;

          nativeBuildInputs = with pkgs; [ autoconf ruby rustc ];
          buildInputs = with pkgs; [ openssl libyaml zlib gmp libffi ];

          # config.guess/config.sub are normally downloaded by the build, which
          # the Nix sandbox forbids; provide them up front.
          postPatch = ''
            cp ${pkgs.gnu-config}/config.guess ${pkgs.gnu-config}/config.sub tool/
          '';

          preConfigure = "./autogen.sh";

          configureFlags = [
            "--enable-zjit"
            "--disable-install-doc"
          ];

          enableParallelBuilding = true;
        };
      });
    };
}
