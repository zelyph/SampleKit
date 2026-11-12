# Install SampleKit

SampleKit has two parts, built from the same source, and a project uses both
at the same version:

- the **command line**, `samplekit`, a program written in Rust. It reads,
  selects, checks and exports your samples, and holds the full-screen
  interface, the TUI;
- the **Python package**, `samplekit`. The command line uses it to run your
  formulas and draw figures, and your own scripts import it.

> **Until 1.0, `pip install samplekit` gives 0.2.0**, the previous version,
> whose API is not this one: always name the version,
> `pip install samplekit==VERSION`. Likewise, the *Latest* release on GitHub
> is 0.2.0, without a command line: this version's archives are under its own
> release, marked *pre-release*.

## The command line

With Rust installed ([rustup](https://rustup.rs) installs it), cargo builds
the command line on any system, `VERSION` being the release's, `1.0.0-rc.3`
for instance:

```console
$ cargo install samplekit --locked --version VERSION
```

`--version` is needed while every version published is a release candidate:
without it, cargo installs only a final release. `--locked` builds with the
dependencies the release was tested with.

### From a release's archive

Without Rust, the command line is one file to download. A release holds an
archive of it for each system, and `SHA256SUMS`, their checksums:

| System | Archive |
| --- | --- |
| Linux, x86-64 | `samplekit-x86_64-unknown-linux-gnu.tar.gz` |
| Linux, ARM | `samplekit-aarch64-unknown-linux-gnu.tar.gz` |
| macOS, Intel | `samplekit-x86_64-apple-darwin.tar.gz` |
| macOS, Apple silicon | `samplekit-aarch64-apple-darwin.tar.gz` |
| Windows | `samplekit-x86_64-pc-windows-msvc.zip` |

Download it from [the releases](https://github.com/zelyph/SampleKit/releases),
with `SHA256SUMS` beside it to check it by, and put `samplekit` (or
`samplekit.exe`) in a folder on your `PATH`, `~/.local/bin` for instance.

On Linux or macOS:

```console
$ sha256sum --check --ignore-missing SHA256SUMS       # shasum -a 256 on macOS
$ mkdir -p ~/.local/bin
$ tar -xzf samplekit-x86_64-unknown-linux-gnu.tar.gz -C ~/.local/bin samplekit
$ samplekit --version
```

`samplekit: command not found` says that `~/.local/bin` is not on your
`PATH`: add `export PATH="$HOME/.local/bin:$PATH"` to your shell's startup
file, `~/.zshrc` or `~/.bashrc`, and open a new terminal.

On macOS, a program downloaded by a browser is held back until you say it
may run: `xattr -d com.apple.quarantine ~/.local/bin/samplekit` says so.

On Windows, the archive is a `.zip`: extract `samplekit.exe` into a folder of
your own, `%LOCALAPPDATA%\Programs\samplekit` for instance, add that folder
to your `PATH` (*Edit environment variables for your account*), and open a new
terminal:

```console
> samplekit --version
```

## The Python package

The Python package is not installed once for the whole machine: it goes into
**each project's own Python environment**, a `.venv/` folder in the project,
which its `.samplekitrc` names: `python = ".venv"`. When SampleKit needs
Python, it runs that one, and never looks for another. `samplekit init`, and
the TUI's start page when it writes the demo ([the home page's example, in
six steps](../index.md)), make that environment and install the package into it from
the Python Package Index, at the version of the command line: the two are
one release, and go together. To do it yourself, with the version
`samplekit --version` says:

```console
$ python -m venv .venv                         # python3 where python is not Python 3
$ .venv/bin/pip install samplekit==VERSION     # 1.0.0-rc.3, for instance
```

On Windows the environment's programs are in `.venv\Scripts\`:
`.venv\Scripts\pip install samplekit==VERSION`.

With [uv](https://docs.astral.sh/uv/), the same environment is made and
filled by uv; when uv is on your `PATH`, `samplekit init` also uses it to
install the package into an environment uv made:

```console
$ uv venv
$ uv pip install samplekit==VERSION
```

The package needs Python 3.11 or later. It works without the command line:
a script that reads, computes and saves samples needs only the package; the
command line is for the terminal and the TUI, and runs the package for your
formulas.

## Tab completion

Tab completion completes commands, options, and the names your project uses:
fields, figures, exports, queries. It is one line in your shell's startup
file, which `samplekit init` names for the shell you use:

| Shell | File | Line |
| --- | --- | --- |
| zsh | `~/.zshrc` | `source <(samplekit completions zsh)` |
| bash | `~/.bashrc` | `source <(samplekit completions bash)` |
| fish | `~/.config/fish/config.fish` | `samplekit completions fish \| source` |
| PowerShell | the file `$PROFILE` names | `samplekit completions powershell \| Out-String \| Invoke-Expression` |
| elvish | `~/.elvish/rc.elv` | `eval (samplekit completions elvish \| slurp)` |

Open a new terminal for it to apply. An alias, `alias sk=samplekit`,
completes as the command does in zsh and fish; bash completes no alias by
itself, and needs one more line after the first, `complete -F
_clap_complete_samplekit sk`. In PowerShell, an alias completes where it is
defined before the line, `Set-Alias sk samplekit` above it in `$PROFILE`.

## Next

`samplekit init` in a folder makes it a project: its `.samplekitrc`, and its
`.venv/` with the Python package ([start a project](start-a-project.md)).
The [home page](../index.md) walks through a project from there.
