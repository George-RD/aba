# Stub hardware-configuration.nix for flake evaluation.
# The real file is host-specific and generated at install time
# by nixos-generate-config or nixos-anywhere.
{ ... }: {
  fileSystems."/" = {
    device = "/dev/disk/by-label/nixos";
    fsType = "ext4";
  };

  boot.loader.grub.device = "/dev/sda";
}
