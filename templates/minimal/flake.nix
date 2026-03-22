{
  description = "My ABA Instance — minimal";

  inputs = {
    aba.url = "github:George-RD/aba";
    nixpkgs.follows = "aba/nixpkgs";
  };

  outputs = { self, aba, nixpkgs, ... }: {
    packages = builtins.mapAttrs (_: v: { default = v.default; }) aba.packages;
  };
}
