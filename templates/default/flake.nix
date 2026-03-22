{
  description = "My ABA Instance — golden path";

  inputs = {
    aba.url = "github:org/aba";
    nixpkgs.follows = "aba/nixpkgs";
  };

  outputs = { self, aba, nixpkgs, ... }: {
    # Inherit the ABA package for this system
    packages = builtins.mapAttrs (_: v: { default = v.default; }) aba.packages;
  };
}
