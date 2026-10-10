import type {
    TableNamingPreset,
    TableNodeNames,
    TableRole,
    TablesSchemaOptions,
} from './TableTypes';
import type { AttrSpec, NodeSpec, SchemaDefinition, SchemaNodeSpec } from './schemaDefinition';
import { defineSchema } from './schemaDefinition';

export const TABLE_NODE_NAMES = {
    prosemirror: {
        table: 'table',
        row: 'table_row',
        cell: 'table_cell',
        headerCell: 'table_header',
    },
    tiptap: { table: 'table', row: 'tableRow', cell: 'tableCell', headerCell: 'tableHeader' },
} as const;

export const DEFAULT_TABLE_NAMING_PRESET: TableNamingPreset = 'prosemirror';

export const TABLE_CELL_COLSPAN_ATTR = 'colspan';
export const TABLE_CELL_ROWSPAN_ATTR = 'rowspan';
export const TABLE_CELL_COLWIDTH_ATTR = 'colwidth';
export const MIN_TABLE_CELL_SPAN = 1;

const TABLE_ROLE_BY_NAME_KEY: ReadonlyArray<[keyof TableNodeNames, TableRole]> = [
    [ 'table', 'table' ],
    [ 'row', 'row' ],
    [ 'cell', 'cell' ],
    [ 'headerCell', 'header_cell' ],
];

const CELL_TABLE_ROLES: readonly TableRole[] = [ 'cell', 'header_cell' ];

function tableCellAttrs(): Record<string, AttrSpec> {
    return {
        [TABLE_CELL_COLSPAN_ATTR]: {
            type: 'number',
            default: MIN_TABLE_CELL_SPAN,
            min: MIN_TABLE_CELL_SPAN,
        },
        [TABLE_CELL_ROWSPAN_ATTR]: {
            type: 'number',
            default: MIN_TABLE_CELL_SPAN,
            min: MIN_TABLE_CELL_SPAN,
        },
        [TABLE_CELL_COLWIDTH_ATTR]: { default: null },
    };
}

function tableSchemaNodeSpecs(names: TableNodeNames): Record<string, SchemaNodeSpec> {
    return {
        [names.table]: {
            content: `${names.row}+`,
            group: 'block',
            role: 'block',
            tableRole: 'table',
            parseDOM: [ { tag: 'table' } ],
            toDOM: [ 'table', 0 ],
        },
        [names.row]: {
            content: `(${names.cell} | ${names.headerCell})*`,
            role: 'block',
            tableRole: 'row',
            parseDOM: [ { tag: 'tr' } ],
            toDOM: [ 'tr', 0 ],
        },
        [names.cell]: {
            content: 'block+',
            attrs: tableCellAttrs(),
            role: 'block',
            tableRole: 'cell',
            parseDOM: [ { tag: 'td' } ],
            toDOM: [ 'td', 0 ],
        },
        [names.headerCell]: {
            content: 'block+',
            attrs: tableCellAttrs(),
            role: 'block',
            tableRole: 'header_cell',
            parseDOM: [ { tag: 'th' } ],
            toDOM: [ 'th', 0 ],
        },
    };
}

function assertUsableTableNames(names: TableNodeNames): void {
    const values = TABLE_ROLE_BY_NAME_KEY.map(([ key ]) => names[key]);

    if (values.some(name => typeof name !== 'string' || name.length === 0)) {
        throw new Error('table node names must all be non-empty strings');
    }

    if (new Set(values).size !== values.length) {
        throw new Error(`table node names must be unique: ${values.join(', ')}`);
    }
}

function cellSpanAttrIsValid(attrs: Record<string, AttrSpec> | undefined, name: string): boolean {
    const spec = attrs?.[name];

    return (
        spec != null &&
        typeof spec.default === 'number' &&
        Number.isInteger(spec.default) &&
        spec.default >= MIN_TABLE_CELL_SPAN
    );
}

function cellDeclaresSpanAttributes(node: NodeSpec): boolean {
    return (
        cellSpanAttrIsValid(node.attrs, TABLE_CELL_COLSPAN_ATTR) &&
        cellSpanAttrIsValid(node.attrs, TABLE_CELL_ROWSPAN_ATTR) &&
        node.attrs != null &&
        Object.prototype.hasOwnProperty.call(node.attrs, TABLE_CELL_COLWIDTH_ATTR)
    );
}

export function tableRolesAreConsistent(schema: SchemaDefinition): boolean {
    const claimedBy = new Map<TableRole, NodeSpec>();

    for (const node of schema.nodes) {
        if (node.tableRole == null) {
            continue;
        }

        if (claimedBy.has(node.tableRole)) {
            return false;
        }

        claimedBy.set(node.tableRole, node);
    }

    if (claimedBy.size === 0) {
        return true;
    }

    if (claimedBy.size !== TABLE_ROLE_BY_NAME_KEY.length) {
        return false;
    }

    return CELL_TABLE_ROLES.every(role => {
        const node = claimedBy.get(role);

        return node != null && cellDeclaresSpanAttributes(node);
    });
}

export function withTablesSchema(
    schema: SchemaDefinition,
    options: TablesSchemaOptions = {}
): SchemaDefinition {
    const names = options.names ?? TABLE_NODE_NAMES[options.preset ?? DEFAULT_TABLE_NAMING_PRESET];
    assertUsableTableNames(names);

    const nodesByName = new Map(schema.nodes.map(node => [ node.name, node ]));
    const namesByRole = new Map<TableRole, string>();

    for (const node of schema.nodes) {
        if (node.tableRole != null) {
            namesByRole.set(node.tableRole, node.name);
        }
    }

    let declared = 0;

    for (const [ key, role ] of TABLE_ROLE_BY_NAME_KEY) {
        const name = names[key];
        const existing = nodesByName.get(name);
        const claimed = namesByRole.get(role);

        if (existing != null && existing.tableRole !== role) {
            throw new Error(`schema already declares node '${name}' without the '${role}' role`);
        }

        if (claimed != null && claimed !== name) {
            throw new Error(`schema already declares node '${claimed}' as the '${role}' role`);
        }

        if (existing != null) {
            declared += 1;
        }
    }

    if (declared === TABLE_ROLE_BY_NAME_KEY.length) {
        return schema;
    }

    if (declared !== 0) {
        throw new Error('schema declares an incomplete set of table node roles');
    }

    return {
        ...schema,
        nodes: [ ...schema.nodes, ...defineSchema({ nodes: tableSchemaNodeSpecs(names) }).nodes ],
    };
}
