// The one Electron API the plugin uses; Obsidian provides the module at runtime.
declare module "electron" {
  export const shell: { openExternal(url: string): Promise<void> };
}
