import {
    DEFAULT_MAX_TABLE_GRID_SLOTS,
    HARD_MAX_TABLE_GRID_SLOTS,
    TABLE_NODE_NAMES,
    prosemirrorSchema,
    tiptapCompatibleSchema,
    withTablesSchema,
    type EditorResourceLimits,
    type NodeSpec,
    type SchemaDefinition,
    type TableNamingPreset,
    type TableNodeNames,
    type TableRole,
    type TablesSchemaOptions,
} from '../index';

const prosemirrorNames: TableNodeNames = TABLE_NODE_NAMES.prosemirror;
const tiptapNames: TableNodeNames = TABLE_NODE_NAMES.tiptap;
void prosemirrorNames;
void tiptapNames;

const presets: TableNamingPreset[] = [ 'prosemirror', 'tiptap' ];
void presets;

const roles: TableRole[] = [ 'table', 'row', 'cell', 'header_cell' ];
void roles;

// @ts-expect-error a table role is not an arbitrary string
const invalidRole: TableRole = 'footer';
void invalidRole;

// @ts-expect-error every table node name is required
const incompleteNames: TableNodeNames = { table: 'table', row: 'table_row' };
void incompleteNames;

const presetOptions: TablesSchemaOptions = { preset: 'tiptap' };
const namedOptions: TablesSchemaOptions = {
    names: { table: 'grid', row: 'gridRow', cell: 'gridCell', headerCell: 'gridHeader' },
};

const defaulted: SchemaDefinition = withTablesSchema(prosemirrorSchema);
const tiptapTabled: SchemaDefinition = withTablesSchema(tiptapCompatibleSchema, presetOptions);
const customTabled: SchemaDefinition = withTablesSchema(prosemirrorSchema, namedOptions);
void defaulted;
void tiptapTabled;
void customTabled;

// @ts-expect-error an unknown preset is rejected
withTablesSchema(prosemirrorSchema, { preset: 'quip' });

const tableNode: NodeSpec | undefined = defaulted.nodes.find(
    node => node.tableRole === 'table'
);
const declaredRole: TableRole | undefined = tableNode?.tableRole;
void declaredRole;

const limits: EditorResourceLimits = {
    maxTableGridSlots: DEFAULT_MAX_TABLE_GRID_SLOTS,
};
const ceiling: number = HARD_MAX_TABLE_GRID_SLOTS;
void limits;
void ceiling;

// @ts-expect-error the grid slot limit is numeric
const invalidLimits: EditorResourceLimits = { maxTableGridSlots: '25000' };
void invalidLimits;
