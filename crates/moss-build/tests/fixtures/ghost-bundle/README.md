# Synthetic Ghost export

Every record is invented. `export.json` uses the Ghost database export shape,
`db[0].data`, with posts, tags, users and their join tables. The companion files
model a signed-in export bundle; tests also wrap this directory in an All data
zip under a prefix and an arbitrary filename.

The fixture contains a public post and page, a subscriber post with a public
preview marker, an unmarked paid post, a draft, a tag, an author, an unused
editor, an image, exact redirects and unsupported redirect sources/targets.
Private sentinels in bodies and account fields make accidental output visible.
The invented reader records must be retained outside the imported project.
