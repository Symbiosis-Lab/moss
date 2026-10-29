# A site's config version is written on purpose

- **Status:** In progress.

## The problem

A site's `.moss/config.toml` carries a `schema_version`. A moss that sees a number above its own refuses to publish the site, because it cannot know what the newer settings mean. That is the right protection, but the number was also written by any moss that merely opened the site: the app, asked to build a site at version 6, saved it back as version 7 with no other change. Released command-line tools then refused to publish it.

The folders that matter most here are shared ones: a site edited by a collaborator on the released app, synced within seconds. One build by a newer app on any machine that touches the folder locks the collaborator out of publishing until they update, and nothing they did caused it.

## The rule

The version in a config is written on purpose, by the person who changes the site, and never by whichever moss happened to open it.

## What the author sees

- Opening, building, previewing and publishing a site never change `config.toml`, and never leave a `config.toml.bak-v*` beside it. Not on a first open by a newer app, not on a site written by an older one.
- Changing a setting writes the file, as it always did. A settings change on a site whose file is in an old shape also brings that shape up to date, once, with a backup next to it. The version it records is the last one that changed what the file looks like, not the newest number this moss knows, so releases from before this moss keep working on the site.
- A designer who needs a newer moss writes `min_moss_version = "0.15.15"`. That is the intentional act that locks out older installs, and the file then says `schema_version = 7`, the first version that has this key. Setting the key through the app's settings writer raises the version to 7 in the same save. Typing it by hand, the designer types both lines; the authoring guide says so.
- Sites already saved as version 7 keep opening, building and publishing.

## Why no upgrade button

An explicit "upgrade this site" action would exist only to write a number, and a number nobody needs is the thing this change removes. A site in an old shape reads correctly without any save, because the shape is brought up to date in memory every time it is read. The first settings change writes the new shape, which is the moment the author is already editing the file. Nothing is stuck waiting for a button.

## Mechanism

Versions up to 6 describe what the file looks like. Version 7 means one thing: the file names a `min_moss_version`, so a moss from before that key refuses it.

- `migrate_to_current` still brings any table to the current version in memory, so every reader sees the current shape. The step from 6 to 7 stays an empty step, which keeps one step per version.
- A save stamps `stamp_for_save`: the version the file already declares or 6, whichever is higher, and 7 when the file has `min_moss_version`. It never lowers a number and never raises it past 6 for a file with no floor.
- `write_managed_toml`, the one door every settings writer goes through, raises a current-shape file with a floor from 6 to 7. A file in an older shape is left to the migrate-and-save step so the shape and the number move together.
- The app's open and build entry no longer calls the saving half of migration. The saving half stays in the app, called by the writers that need a current-shape file before they edit it.

## The one-time steps that edit `.moss/.gitignore`

The steps to version 5 and 6 rewrite a second file, and the version number was the record of "this has run": a prune that ran twice would remove a line the author added back. With builds no longer saving the number, running them at every build would repeat them.

They now run in the same explicit save, gated on the version the file declared before it, and the version they leave behind (6) is the record. A site in an old shape that is only built keeps its old `.gitignore` until the author changes a setting. What is delayed is cosmetic: a `data/` line git cannot re-include through, and publish records that do not yet reach git.

Considered and rejected: running them as idempotent checks at every build. The first (`data/` to `data/*`) would be safe, since both spellings ignore the same files, but the prune is not: it deletes a line moss once wrote, and an author may have written the same line on purpose since.

## Left open

A home page's `analytics:` front matter used to be copied into `config.toml` the first time the app built the site. It is now read in memory by the services reader (`get_services_config`), and the settings screen saves it on an explicit edit.

Other settings writers edit the table as read, not migrated, so a very old config (before the services layout) is still edited in its old shape by them. Only the channel writer saves the migrated shape first.
