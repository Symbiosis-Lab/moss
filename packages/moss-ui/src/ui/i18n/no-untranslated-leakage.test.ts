import { UI_STRINGS } from './ui-strings';
import { guardStringTables, type StringTables } from './__tests__/string-table-guard';

guardStringTables({ 'ui-strings': UI_STRINGS as unknown as StringTables[string] });
