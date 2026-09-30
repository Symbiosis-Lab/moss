# moss for Obsidian

Build and preview the current vault as a website with [moss](https://mosspub.com), without leaving Obsidian. Requires the moss CLI (`npm i -g @symbiosis-lab/moss` or Homebrew).

## Install with BRAT

This plugin isn't in Obsidian's community plugin directory yet, so install it with [BRAT](https://github.com/TfTHacker/obsidian42-brat) (Beta Reviewer's Auto-update Tester).

1. Install and enable the BRAT plugin from Obsidian's community plugins.
2. Run BRAT's "Add a beta plugin with frozen version" command (not the plain "Add a beta plugin" one — see below).
3. Enter the repository as `Symbiosis-Lab/moss`.
4. Enter the release tag, for example `obsidian-moss-v0.1.0`.

Use the frozen-version command, not "latest": this repository also ships other products under plain `vX.Y.Z` tags, so the repository's overall latest release is often not a plugin release at all. The plugin's own releases are always tagged `obsidian-moss-v<version>`, matching the `version` in `manifest.json`.
