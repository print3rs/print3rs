# Print3rs
### A rusty kind of 3D printer host toolkit

The goal of this repo is to provide functionality on par with, and eventually exceeding the popular python toolkit [Printrun](https://www.pronterface.com/).

Initially, this means:
* A Rust library to ease building a 3D printer host application
* A cross-platform cli/console utility to talk to a 3D printer
* A cross-platform GUI with customizable UI to interact with 3D printers

Eventually, we would like to implement:
* Unified tooling to talk over USB/Serial, Wifi/TCP, Bluetooth, or anything else!
* g-code slicing
* An embedded (through browser) Printer UI
* More complex print staging
* Non 3D-printer CNC machine integration

## !!! Under Active development !!!
Any interfaces in any of the crates are subject to radical breaking changes without notice.
User binaries could have very different semantics from commit to commit.

Until a reasonable 0.1 is met, don't use anything in this repo in other projects!

In the mean time, testing using a tagged release, any code reviewing, or contributions are accepted :D

## Licencing
All _library_ code is permissively licensed under MIT, making it compatible with almost any codebase, and a no-brainer to bring in from Cargo where needed

All _application_ code is licenced under GPLv3, so if you want to use the existing console or GUI directly, you will have to adopt GPL. 

All _documentation_ or non-code related artifacts are Public Domain, unless otherwise specified.

This gives flexibility for anyone to build their own hosts based on the libraries, but if you want to skip that work,
we ask that you keep those changes open and share your improvements.

<!-- gen-readme start - generated by https://github.com/jetpack-io/devbox/ -->
## Getting Started
This project uses [devbox](https://github.com/jetpack-io/devbox) to manage its development environment.

Install devbox:
```sh
curl -fsSL https://get.jetpack.io/devbox | bash
```

Start the devbox shell:
```sh 
devbox shell
```

Run a script in the devbox environment:
```sh
devbox run <script>
```
## Scripts
Scripts are custom commands that can be run using this project's environment. This project has the following scripts:

* [build](#devbox-run-build)
* [build-debug](#devbox-run-build-debug)
* [run-console](#devbox-run-run-console)
* [run-gui](#devbox-run-run-gui)
* [test](#devbox-run-test)

## Shell Init Hook
The Shell Init Hook is a script that runs whenever the devbox environment is instantiated. It runs 
on `devbox shell` and on `devbox run`.
```sh
projectDir=.
rustupHomeDir="$projectDir"/.rustup
mkdir -p $rustupHomeDir
export RUSTUP_HOME=$rustupHomeDir
export LIBRARY_PATH=$LIBRARY_PATH:"$projectDir/nix/profile/default/lib"
rustup default stable
cargo fetch
```

## Packages

* [rustup@latest](https://www.nixhub.io/packages/rustup)
* [libiconv@latest](https://www.nixhub.io/packages/libiconv)
* [libudev-zero@latest](https://www.nixhub.io/packages/libudev-zero)

## Script Details

### devbox run build
```sh
cargo build --release
```
&ensp;

### devbox run build-debug
```sh
cargo build
```
&ensp;

### devbox run run-console
```sh
cargo run -p print3rs-console
```
&ensp;

### devbox run run-gui
```sh
cargo run -p print3rs-gui
```
&ensp;

### devbox run test
```sh
cargo test -- --show-output
```
&ensp;



<!-- gen-readme end -->
