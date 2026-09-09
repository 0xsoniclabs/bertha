# IFI Setup

## Tooling

```sh
# fish
wget https://github.com/fish-shell/fish-shell/releases/download/4.9.3/fish-4.9.3-linux-x86_64.tar.xz
tar -xf fish-4.9.3-linux-x86_64.tar.xz
mv fish .local/bin/

# rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
cargo install eza bottom du-dust

# go
wget https://go.dev/dl/go1.27.1.linux-amd64.tar.gz
tar -xzf go1.27.1.linux-amd64.tar.g

# clang 23
wget https://github.com/llvm/llvm-project/releases/download/llvmorg-23.1.1/LLVM-23.1.1-Linux-X64.tar.xz
tar -xf LLVM-23.1.1-Linux-X64.tar.xz -C ~/.local/
mv ~/.local/LLVM-23.1.1-Linux-X64 ~/.local/llvm-23.1.1

# protoc
wget https://github.com/protocolbuffers/protobuf/releases/download/v36.1/protoc-36.1-linux-x86_64.zip
python3 -m zipfile -e protoc-36.1-linux-x86_64.zip ~/.local/
chmod +x ~/.local/bin/protoc
protoc --version

# rocksdb
git clone --depth 1 --branch v1.8.12 https://github.com/linxGnu/grocksdb
sed -i 's/-DROCKSDB_BUILD_SHARED=OFF/-DROCKSDB_BUILD_SHARED=ON/' ~/grocksdb/build.sh
bash ~/grocksdb/build.sh $HOME/.local/rocksdb-8.9.1
```

## Config

`.profile`
```sh
# Read by login shells: your ssh session, and the non-interactive
# `#!/bin/bash -l` Slurm job scripts. Everything the jobs need must be set
# here, and above the fish block, which replaces the shell and never returns.

. "$HOME/.cargo/env"

export PATH="$HOME/.local/bin:$HOME/.local/go/bin:$HOME/.local/llvm-23.1.1/bin:$PATH"

# RocksDB 8.9.1 for grocksdb, built into a prefix since there is no sudo.
export CGO_CFLAGS="-I$HOME/.local/rocksdb-8.9.1/include"
export CGO_LDFLAGS="-L$HOME/.local/rocksdb-8.9.1/lib -L$HOME/.local/rocksdb-8.9.1/lib64"

export ROCKSDB_LIB_DIR="$HOME/.local/rocksdb-8.9.1/lib"
export CGO_CFLAGS="-I$HOME/.local/rocksdb-8.9.1/include"
export CGO_LDFLAGS="-L$HOME/.local/rocksdb-8.9.1/lib -L$HOME/.local/rocksdb-8.9.1/lib64"
export LD_LIBRARY_PATH="$HOME/.local/rocksdb-8.9.1/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"

# Hand interactive login shells over to fish. The `case $- in *i*` guard is
# what keeps batch jobs in bash, and FISH_LAUNCHED prevents a re-exec loop.
# The BASH_VERSION check matters because .profile is also read by plain sh.
if [ -n "${BASH_VERSION-}" ] && [ -x "$HOME/.local/bin/fish" ] && [ -z "${FISH_LAUNCHED-}" ]; then
    case $- in
        *i*) export FISH_LAUNCHED=1; exec "$HOME/.local/bin/fish" ;;
    esac
fi
```

`.bashrc`
```sh
# Read by interactive non-login shells only — login shells read .profile
# instead and never source this file.

# Hand those shells over to fish as well. This is the path taken by
# `srun --pty bash -i`, which is interactive but not a login shell, so the
# block in .profile does not fire for it. No exports here: Slurm passes the
# submitting environment through, so PATH and CGO_* are already inherited.
if [[ $- == *i* ]] && [[ -x "$HOME/.local/bin/fish" ]] && [[ -z "$FISH_LAUNCHED" ]]; then
    export FISH_LAUNCHED=1
    exec "$HOME/.local/bin/fish"
fi
```

add to `.tmux.conf`
```sh
set -g default-shell /home/lorenz.schueler/.local/bin/fish
```

## Repos

```sh
git clone https://github.com/0xsoniclabs/bertha
git clone https://github.com/0xsoniclabs/tosca
cd tosca; git submodule update --init --recursive
```
