# c10r completions

Print a shell completion script.

```sh
c10r completions zsh
```

## Arguments

| Argument | Meaning                                                                                                                  |
| -------- | ------------------------------------------------------------------------------------------------------------------------ |
| `SHELL`  | One of `bash`, `elvish`, `fish`, `powershell`, `zsh`. Required; an unsupported value is rejected with the supported set. |

## Options

`completions` takes only the [common flags](common.md#shared-flags): `--db`, `--workspace`, `--json`, `--color`.

## Installing

zsh — add `~/.zfunc` to `fpath` before `compinit` runs, then start a new shell:

```sh
c10r completions zsh > ~/.zfunc/_c10r
```

bash:

```sh
c10r completions bash > ~/.local/share/bash-completion/completions/c10r
# or, in ~/.bashrc:
source <(c10r completions bash)
```

For `fish`, `elvish`, and `powershell`, write the script into that shell's own completion path.

Regenerate after upgrading `c10r`.
