import {
    type RichTextEditorProps,
    type RichTextEditorRef,
    type Selection,
    type TableCellSelection,
    type TableCommand,
    type TableDirection,
} from '../index';

declare const editor: RichTextEditorRef;

const pending: Promise<void> = editor.runTableCommand({ type: 'insertTable' });
void pending;

const commands: TableCommand[] = [
    { type: 'insertTable' },
    { type: 'insertTable', rows: 2, columns: 3, withHeaderRow: false },
    { type: 'deleteTable' },
    { type: 'addTableRow', side: 'before' },
    { type: 'addTableRow', side: 'after' },
    { type: 'deleteTableRows' },
    { type: 'addTableColumn', side: 'before' },
    { type: 'deleteTableColumns' },
    { type: 'toggleTableHeader', target: 'row' },
    { type: 'toggleTableHeader', target: 'column' },
    { type: 'toggleTableHeader', target: 'cell' },
    { type: 'mergeTableCells' },
    { type: 'splitTableCell' },
    { type: 'resizeTableColumn', tablePos: 0, column: 1, width: 120 },
    { type: 'selectTableCells', anchorCell: 2, headCell: 16 },
    { type: 'selectTableRows' },
    { type: 'selectTableColumns' },
    { type: 'clearTableCells' },
    { type: 'goToTableCell', direction: 1, appendRow: true },
    { type: 'goToTableCell', direction: -1, appendRow: false },
];
void commands;

// @ts-expect-error the engine's internal resize discriminant is not public
const engineResize: TableCommand = { type: 'setTableColumnWidth', width: 120 };
void engineResize;

// @ts-expect-error a resize names its table, column and width
const untargetedResize: TableCommand = { type: 'resizeTableColumn', width: 120 };
void untargetedResize;

// @ts-expect-error rows and columns are the only insertion edges
const middleRow: TableCommand = { type: 'addTableRow', side: 'middle' };
void middleRow;

// @ts-expect-error a table header target is a row, column or cell
const tableHeader: TableCommand = { type: 'toggleTableHeader', target: 'table' };
void tableHeader;

// @ts-expect-error cell navigation moves one cell at a time
const skipCell: TableCommand = { type: 'goToTableCell', direction: 2, appendRow: true };
void skipCell;

// @ts-expect-error callers never supply a transaction origin
const originated: TableCommand = { type: 'mergeTableCells', origin: 'localCommand' };
void originated;

// @ts-expect-error cell selections carry document positions, not strings
const stringCell: TableCommand = { type: 'selectTableCells', anchorCell: '2', headCell: 16 };
void stringCell;

const directions: TableDirection[] = [ 'ltr', 'rtl' ];
void directions;

const rtlHost: Pick<RichTextEditorProps, 'tableDirection'> = { tableDirection: 'rtl' };
void rtlHost;

// @ts-expect-error the table direction is ltr or rtl only
const autoHost: Pick<RichTextEditorProps, 'tableDirection'> = { tableDirection: 'auto' };
void autoHost;

const cellSelection: TableCellSelection = { type: 'cell', anchorCell: 2, headCell: 16 };
const reported: Selection = cellSelection;
void reported;
