// Inline in the head, so a signed-in reader's "Subscribed" is there on the first frame; the form itself is bound by forms.js.
import { watchReader } from './reader.js';

watchReader();
