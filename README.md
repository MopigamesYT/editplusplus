# ![Application Icon for Edit](./assets/edit.svg) edit++

A pluggable fork of [Microsoft Edit](https://github.com/microsoft/edit).

Upstream Edit pays homage to the classic [MS-DOS Editor](https://en.wikipedia.org/wiki/MS-DOS_Editor) with a modern interface and VS Code-like input controls. edit++ keeps all of that and makes the editor extensible: every action is a named command, every key is rebindable, and plugins can add their own.

The executable is named `epp`, so it sits alongside an existing `edit` or `msedit` install rather than replacing it.

![Screenshot of Edit with the About dialog in the foreground](./assets/edit_hero_image.png)

## What this fork adds

* **A command registry.** Every action the editor can perform has a dotted name such as `file.save` or `view.goToFile`. Menus, keybindings and plugins all go through it.
* **A command palette.** `Ctrl+Shift+P` lists every command with its binding, filtered fuzzily.
* **Rebindable keys, including chords.** Bindings live in `settings.json` and can span several keypresses, so a leader key works.
* **A which-key hint.** Start a chord and a panel shows what could complete it.
* **Plugins.** See [docs/PLUGINS.md](docs/PLUGINS.md).

## Configuration

Settings live in `settings.json` under:

Platform | Path
--- | ---
Linux / other | `$XDG_CONFIG_HOME/epp/` or `~/.config/epp/`
macOS | `~/Library/Application Support/edit++/`
Windows | `%APPDATA%\edit++\`

Open it from the editor with **File → Preferences**, or `Ctrl+K F P`.

If you are coming from Microsoft Edit and have no `settings.json` here yet, edit++ reads the upstream one instead, so your existing settings keep working.

### Keybindings

```jsonc
{
  // The chord prefix. Defaults to Ctrl+K.
  "keyboard.leader": "ctrl+k",

  "keyboard.bindings": {
    "ctrl+shift+p": "view.commandPalette",
    "<leader>ff":   "view.goToFile",
    "ctrl+k ctrl+s": "file.save",

    // false or null removes a default binding.
    "ctrl+w": false
  }
}
```

Both spellings parse and can be mixed: `ctrl+shift+p` and `<C-S-p>` mean the same thing. Chords are written as space-separated keys, or as characters following `<leader>`. Named keys are `f1`-`f12`, `esc`, `tab`, `enter`, `space`, `backspace`, `delete`, `insert`, `home`, `end`, `pageup`, `pagedown`, and the arrows.

Only letters, digits, space and those named keys can be bound. Punctuation such as `/` cannot, because terminals do not report it as a distinct key. A binding that cannot be parsed is reported in the error dialog at startup rather than being ignored.

Run `config.reload` (`Ctrl+K C R`) to apply changes without restarting.

### Default bindings

Chord | Command
--- | ---
`Ctrl+K Ctrl+K` | Command palette
`Ctrl+K F F` | Go to file
`Ctrl+K F N` | New file
`Ctrl+K F S` | Save
`Ctrl+K F P` | Preferences
`Ctrl+K U W` | Toggle word wrap
`Ctrl+K C R` | Reload configuration
`Ctrl+K H A` | About

Everything Microsoft Edit bound before, such as `Ctrl+S` and `Ctrl+P`, is unchanged.

## Installation

[![Packaging status](https://repology.org/badge/vertical-allrepos/microsoft-edit.svg?exclude_unsupported=1)](https://repology.org/project/microsoft-edit/versions)

You can also download binaries from [our Releases page](https://github.com/microsoft/edit/releases/latest).

### Windows

You can install the latest version with WinGet:
```powershell
winget install Microsoft.Edit
```

### Linux (build from source)

If your distribution does not provide binaries, or if you'd like to build your own, you can use our install script, provided you have installed:
* Rust (via `rustup` or similar)
* A C compiler (e.g. `gcc`)
* ICU (e.g. libicu78, libicu, icu)
* curl/wget and tar

The following command will then install `epp` into `~/.local/bin`:
```sh
curl --proto '=https' --tlsv1.2 -sSf https://raw.githubusercontent.com/microsoft/edit/main/assets/install.sh | sh
```

Additional flags are `--dev`, to build directly from the main branch, and `--system` to install into `/usr/local/bin`. For instance:
```sh
curl --proto '=https' --tlsv1.2 -sSf https://raw.githubusercontent.com/microsoft/edit/main/assets/install.sh | sh -s -- --dev --system
```

### macOS

You can install the latest version with Homebrew:
```sh
brew install msedit
```

## Build Instructions

* [Install Rust](https://www.rust-lang.org/tools/install)
* Clone the repository
* If you're using nightly Rust:
  ```sh
  cargo build --release --config .cargo/release.toml
  ```
* If you're using stable Rust:
  * Ideally: Set the environment variable `RUSTC_BOOTSTRAP=1` and use the **nightly** build instructions above.
    This is recommended, because it drastically reduces the binary size and slightly improves performance.
  * Otherwise, simply run:
    ```sh
    cargo build --release
    ```

### Build Configuration

You can set the following environment variables at build-time to configure the build:

Environment variable | Description
--- | ---
`EDIT_CFG_ICU*` | See [ICU library name (SONAME)](#icu-library-name-soname) below for details. Linux package maintainers are advised to review and configure these options.
`EDIT_CFG_LANGUAGES` | A comma-separated list of languages to include in the build. See [i18n/edit.toml](i18n/edit.toml) for available languages.

## Notes to Package Maintainers

### Package Naming

The canonical executable name is `epp`. Do not name it `edit` or `msedit`: those belong to upstream Microsoft Edit, and edit++ is built to be installed alongside it rather than to replace it.

### ICU library name (SONAME)

This project optionally depends on the ICU library for its Search and Replace functionality.

By default, the project will look for the following library names:

 Variable | Windows | macOS | Linux / Other
----------|---------|-------|---------------
`EDIT_CFG_ICUUC_SONAME` | `icuuc.dll` | `libicucore.dylib` | `libicuuc.so`
`EDIT_CFG_ICUI18N_SONAME` | `icuin.dll` | `libicucore.dylib` | `libicui18n.so`

If your installation uses a different SONAME, please set the following environment variable at build time:
* `EDIT_CFG_ICUUC_SONAME`:
  For instance, `libicuuc.so.76`.
* `EDIT_CFG_ICUI18N_SONAME`:
  For instance, `libicui18n.so.76`.

Additionally, this project assumes that the ICU exports symbols without `_` prefix and without version suffix, such as `u_errorName`.
If your installation uses versioned exports, please set:
* `EDIT_CFG_ICU_CPP_EXPORTS`:
  If set to `true`, it'll look for C++ symbols such as `_u_errorName`.
  Enabled by default on macOS.
* `EDIT_CFG_ICU_RENAMING_VERSION`:
  If set to a version number, such as `76`, it'll look for symbols such as `u_errorName_76`.

Finally, you can set the following environment variables:
* `EDIT_CFG_ICU_RENAMING_AUTO_DETECT`:
  If set to `true`, the executable will try to detect the `EDIT_CFG_ICU_RENAMING_VERSION` value at runtime.
  The way it does this is not officially supported by ICU and as such is not recommended to be relied upon.
  Enabled by default on UNIX (excluding macOS) if no other options are set.

To test your build settings, run `cargo test` with the `--ignored` flag. For instance:
```sh
cargo test -- --ignored
```
