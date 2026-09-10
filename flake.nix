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

        # The bundled gems named in gems/bundled_gems, prefetched from
        # rubygems.org as a fixed-output derivation (network is allowed there).
        # When the gem list changes, the build fails with a hash mismatch that
        # names the new outputHash to paste in below.
        bundled-gems = pkgs.runCommand "ruby-bundled-gems"
          {
            nativeBuildInputs = [ pkgs.curl pkgs.cacert ];
            outputHashAlgo = "sha256";
            outputHashMode = "recursive";
            outputHash = "sha256-Nme+RNqL71r2i1hWyVlx2kuSjsWTUsrQkQ0a/l6SH+8=";
            SSL_CERT_FILE = "${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt";
          } ''
            mkdir $out
            grep -Ev '^[[:space:]]*(#|$)' ${self}/gems/bundled_gems |
            while read -r name ver url rev; do
              curl -fsSL --retry 3 -o "$out/$name-$ver.gem" \
                "https://rubygems.org/downloads/$name-$ver.gem"
            done
          '';

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
            # The build normally downloads bundled gems from rubygems.org,
            # which the sandbox forbids. Pre-place the prefetched .gem files
            # and drop the git revision pins so make does not try to clone
            # and build those gems from source.
            cp ${bundled-gems}/*.gem gems/
            chmod +w gems/*.gem
            awk '/^[[:space:]]*#/ || NF < 4 { print; next } { print $1, $2, $3 }' \
              gems/bundled_gems > gems/bundled_gems.tmp
            mv gems/bundled_gems.tmp gems/bundled_gems
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
