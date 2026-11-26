import fs from "node:fs";
import path from "node:path";

/** Removes every file under siteDir that `named` does not list, so a renamed
 *  or dropped page is not built. True when it removed one.
 *
 *  Dot directories are never entered: another process may be building this
 *  site and deleting `.moss/cache` while the walk runs. Kept dependency-free
 *  so plain node can import it in a test.
 *  @param {string} siteDir
 *  @param {Iterable<string>} named
 *  @returns {boolean} */
export function pruneUnnamed(siteDir, named) {
  if (!fs.existsSync(siteDir)) return false;
  const keep = new Set([...named].map((rel) => path.normalize(rel)));
  let pruned = false;
  /** @param {string} rel */
  const walk = (rel) => {
    for (const entry of fs.readdirSync(path.join(siteDir, rel), { withFileTypes: true })) {
      // moss's build writes the folder's Finder stamp here; pruning it would
      // drop the build stamp and rebuild a folder its server already owns.
      if (entry.name.startsWith(".") || (rel === "" && entry.name === "Icon\r")) continue;
      const child = path.join(rel, entry.name);
      if (entry.isDirectory()) walk(child);
      else if (entry.isFile() && !keep.has(child)) {
        fs.rmSync(path.join(siteDir, child));
        pruned = true;
      }
    }
  };
  walk("");
  return pruned;
}
