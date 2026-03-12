# toss

throw and catch style moving and copying program

## Why?

I love using the terminal and I normally use `cp` and `mv` to copy and move files between folders but when refactoring/cleaning the directories I want to organize files to respective folders in different places. Normally the Downloads folder. While doing that I don't want to type the path multiple times just to move files to a particular location from different places.

What I thought is.. why not just run a catcher in a destination path, and throw files from anywhere in the system. And when throwing the files, why not make it able to move some files and copy other files. This program does exactly that.

## Usage

Run `toss` in the destination directory to start receiving, then run `toss <files>` from anywhere to send files.
> Run `toss --help` for all arguments and options.

```sh
# Start the server (in destination directory):
toss

# Send files (from another terminal):
toss <files>
```

# Installation

## For Normal Distros

```sh
# should be good for most systems
cargo install toss
```

## NixOS

There are two ways to do it:

### Via NixOS configuration

```nix
{
  inputs = {
    nixpkgs.url = "nixpkgs/nixos-unstable";
    toss.url = "github:scientiac/toss";
  };

  outputs = { self, nixpkgs, toss, ... }: {
    nixosConfigurations.your-hostname = nixpkgs.lib.nixosSystem {
      system = "x86_64-linux";
      modules = [
        ./configuration.nix
        toss.nixosModules.default
        {
          programs.toss.enable = true;
        }
      ];
    };
  };
}
```

### Via Home Manager configuration

```nix
{
  inputs = {
    nixpkgs.url = "nixpkgs/nixos-unstable";
    home-manager.url = "github:nix-community/home-manager";
    toss.url = "github:scientiac/toss";
  };

  outputs = { self, nixpkgs, home-manager, toss, ... }: {
    homeConfigurations.your-username = home-manager.lib.homeManagerConfiguration {
      pkgs = nixpkgs.legacyPackages.x86_64-linux;
      modules = [
        toss.homeManagerModules.default
        {
          programs.toss.enable = true;
        }
      ];
    };
  };
}
```
