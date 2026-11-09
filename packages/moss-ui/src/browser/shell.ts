/**
 * The shell frame's entry: fills the host record, then starts the chrome and
 * the preview.
 */
import * as carrier from './carrier';
import { fillBrowserHost } from './host';
import { portBus, receivePort } from './port-bus';
import { shellHost, type ServeConfig } from './shell-host';

async function main(): Promise<void> {
  const port = await receivePort();
  const config = (await (await fetch('/__moss/config', { credentials: 'same-origin' })).json()) as ServeConfig;
  const boot = await (carrier.call('editor_bootstrap', { sourcePath: null }) as Promise<{
    project_path: string | null;
    root_name: string | null;
  }>).catch((e: unknown) => (e instanceof carrier.SignedOut ? null : Promise.reject(e)));
  const folder = boot?.project_path ? { path: boot.project_path, name: boot.root_name } : null;
  fillBrowserHost(portBus(port), folder ? shellHost(config, folder) : {});
  // The UI's modules load only after the record is filled.
  const { startShell } = await import('./shell-app');
  await startShell(boot ? folder : 'signed-out', config);
}

void main();
