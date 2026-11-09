/** The editor frame's entry: fills the host record, then starts the editor. */
import { fillBrowserHost } from './host';
import { portBus, receivePort } from './port-bus';

fillBrowserHost(portBus(await receivePort()));
// The UI's modules load only after the record is filled.
await import('../ui/editor/editor-main');
