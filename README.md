# Seer

Replay Solana transactions. Fork state. Inspect programs, registers, and path conditions.

## Install a release binary

macOS and Linux:

```
curl -sSfL https://github.com/SeerApp/seer/releases/latest/download/install.sh | sh
```

Windows (PowerShell):

```
irm https://github.com/SeerApp/seer/releases/latest/download/install.ps1 | iex
```

That downloads one archive for your OS and CPU: `seer` (`seer.exe` on Windows) plus the `libz3` that archive was linked against. Unix `install.sh` maps `uname` to `linux|darwin|windows` + `amd64|arm64` and installs into a writable `$HOME/.local/bin` or `/usr/local/bin`. Windows `install.ps1` maps `$env:PROCESSOR_ARCHITECTURE` and installs into `%LOCALAPPDATA%\seer`. The library is unpacked next to the binary in that same directory. Do not install Z3 separately for this path.

Prefix:

```
curl -sSfL https://github.com/SeerApp/seer/releases/latest/download/install.sh | sh -s -- --prefix /usr/local/seer
```

```
& .\install.ps1 -Prefix "$env:LOCALAPPDATA\seer"
```

Linux x86_64 builds of Z3 4.15.3 need glibc 2.39 or newer. Ubuntu 24.04 is fine. Ubuntu 22.04 is not.

## Build from source

Install Z3, then cargo.

macOS:

```
brew install z3 pkg-config
```

Debian / Ubuntu:

```
sudo apt install libz3-dev pkg-config
```

Windows: unpack the zip named in `z3-pin` for `x86_64-pc-windows-msvc`. Set `Z3_SYS_Z3_HEADER` to that `include\z3.h` and `Z3_LIBRARY_PATH_OVERRIDE` to the directory with `libz3.lib` / `libz3.dll`.

Same zip on Unix if you want CI's bits:

```
export Z3_SYS_Z3_HEADER=/path/to/include/z3.h
export Z3_LIBRARY_PATH_OVERRIDE=/path/to/bin
```

Build (from this repo; cargo fetches the Agave / LiteSVM / SBPF git deps):

```
cargo build -p seer --release
```

The binary you just built talks to that Z3. It is not the GitHub archive. Do not run `install.sh` / `install.ps1` against it.

Z3 4.15.x matches the current `z3` 0.20.2 crate. `Cargo.lock` does not pin Microsoft Z3.

## glassbox

`seer glassbox` needs a working libz3. A release install already has it next to `seer`. A source build uses the Z3 you installed above.