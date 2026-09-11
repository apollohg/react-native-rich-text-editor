import { createHash } from 'crypto';

import { NativeEditorBoundaryError } from '../NativeEditorBoundaryError';
import {
    DEFAULT_MAX_TABLE_GRID_SLOTS,
    HARD_MAX_TABLE_GRID_SLOTS,
    resolveEditorResourceLimits,
} from '../ResourceLimits';
import { TABLE_NODE_NAMES, withTablesSchema } from '../TableSchema';
import type { TableNodeNames, TableRole } from '../TableTypes';
import { acceptingContentSymbols, minimalContentMatch } from '../contentExpression';
import type { NodeSpec, SchemaDefinition } from '../schemaDefinition';
import { defaultSchema, prosemirrorSchema, tiptapCompatibleSchema } from '../schemaPresets';
import { resolveDocumentSchema } from '../schemaResolution';

const TABLES_FREE_CANONICAL_SHA256: Readonly<Record<string, string>> = {
    prosemirror: '1f23f42132387529c4910933f6fae78e2c89e49d1afd96dd0aa37df81ddaf745',
    tiptap: '37dfe3f23abda048076c5ea4327c290863e364b5572370a3cb8f79c5857c14e0',
};

const TABLES_FREE_PRESETS: ReadonlyArray<[string, SchemaDefinition]> = [
    [ 'prosemirror', prosemirrorSchema ],
    [ 'tiptap', tiptapCompatibleSchema ],
];

const GRID_LIMIT_FIELD = 'maxTableGridSlots';

function canonicalJson(schema: SchemaDefinition): string {
    return JSON.stringify(resolveDocumentSchema(schema));
}

function nodeNamed(schema: SchemaDefinition, name: string): NodeSpec {
    const node = schema.nodes.find(candidate => candidate.name === name);

    if (node == null) {
        throw new Error(`schema declares no node '${name}': ${schema.nodes.map(n => n.name)}`);
    }

    return node;
}

function nodeNamesWithRole(schema: SchemaDefinition, role: TableRole): string[] {
    return schema.nodes.filter(node => node.tableRole === role).map(node => node.name);
}

function catchThrown(run: () => unknown): unknown {
    try {
        run();
    } catch (error) {
        return error;
    }

    throw new Error('expected the call to throw');
}

describe('tables-free schema fingerprint preservation', () => {
    it.each(TABLES_FREE_PRESETS)(
        'keeps the %s preset canonical serialization byte-identical',
        (name, schema) => {
            const json = canonicalJson(schema);

            expect(createHash('sha256').update(json).digest('hex')).toBe(
                TABLES_FREE_CANONICAL_SHA256[name]
            );
        }
    );

    it.each(TABLES_FREE_PRESETS)('emits no tableRole key for the %s preset', (_name, schema) => {
        expect(canonicalJson(schema)).not.toContain('tableRole');
    });

    it('leaves the source schema untouched when tables are added', () => {
        const before = JSON.stringify(prosemirrorSchema);
        withTablesSchema(prosemirrorSchema);

        expect(JSON.stringify(prosemirrorSchema)).toBe(before);
        expect(canonicalJson(prosemirrorSchema)).toBe(
            JSON.stringify(resolveDocumentSchema(prosemirrorSchema))
        );
    });
});

describe('withTablesSchema naming presets', () => {
    it('defaults to the ProseMirror preset names', () => {
        const schema = withTablesSchema(prosemirrorSchema);

        expect(nodeNamesWithRole(schema, 'table')).toEqual([ TABLE_NODE_NAMES.prosemirror.table ]);
        expect(nodeNamesWithRole(schema, 'row')).toEqual([ TABLE_NODE_NAMES.prosemirror.row ]);
        expect(nodeNamesWithRole(schema, 'cell')).toEqual([ TABLE_NODE_NAMES.prosemirror.cell ]);
        expect(nodeNamesWithRole(schema, 'header_cell')).toEqual([
            TABLE_NODE_NAMES.prosemirror.headerCell,
        ]);
    });

    it('uses the Tiptap preset names when asked', () => {
        const schema = withTablesSchema(tiptapCompatibleSchema, { preset: 'tiptap' });

        expect(nodeNamesWithRole(schema, 'table')).toEqual([ TABLE_NODE_NAMES.tiptap.table ]);
        expect(nodeNamesWithRole(schema, 'row')).toEqual([ TABLE_NODE_NAMES.tiptap.row ]);
        expect(nodeNamesWithRole(schema, 'cell')).toEqual([ TABLE_NODE_NAMES.tiptap.cell ]);
        expect(nodeNamesWithRole(schema, 'header_cell')).toEqual([
            TABLE_NODE_NAMES.tiptap.headerCell,
        ]);
    });

    it.each([ 'prosemirror', 'tiptap' ] as const)(
        'produces a resolvable %s-named schema keeping every table role',
        preset => {
            const source = preset === 'tiptap' ? tiptapCompatibleSchema : prosemirrorSchema;
            const schema = withTablesSchema(source, { preset });
            const resolved = resolveDocumentSchema(schema);

            expect(resolved).not.toBe(defaultSchema);
            expect(nodeNamesWithRole(resolved, 'table')).toEqual([
                TABLE_NODE_NAMES[preset].table,
            ]);
            expect(nodeNamesWithRole(resolved, 'header_cell')).toEqual([
                TABLE_NODE_NAMES[preset].headerCell,
            ]);
        }
    );

    it('accepts unique custom node-role names', () => {
        const names: TableNodeNames = {
            table: 'grid',
            row: 'gridRow',
            cell: 'gridCell',
            headerCell: 'gridHeader',
        };
        const schema = withTablesSchema(prosemirrorSchema, { names });
        const resolved = resolveDocumentSchema(schema);

        expect(resolved).not.toBe(defaultSchema);
        expect(nodeNamed(resolved, 'grid').tableRole).toBe('table');
        expect(nodeNamed(resolved, 'gridHeader').tableRole).toBe('header_cell');
    });

    it('returns the same schema when applied twice', () => {
        const once = withTablesSchema(prosemirrorSchema);
        const twice = withTablesSchema(once);

        expect(twice).toBe(once);
    });
});

describe('withTablesSchema conflict rejection', () => {
    it('rejects duplicate custom names', () => {
        expect(() =>
            withTablesSchema(prosemirrorSchema, {
                names: {
                    table: 'grid',
                    row: 'gridRow',
                    cell: 'gridCell',
                    headerCell: 'gridCell',
                },
            })).toThrow(/unique/);
    });

    it('rejects empty custom names', () => {
        expect(() =>
            withTablesSchema(prosemirrorSchema, {
                names: { table: '', row: 'gridRow', cell: 'gridCell', headerCell: 'gridHeader' },
            })).toThrow();
    });

    it('rejects a second naming preset over an already-tabled schema', () => {
        const tabled = withTablesSchema(prosemirrorSchema);

        expect(() => withTablesSchema(tabled, { preset: 'tiptap' })).toThrow();
    });

    it('rejects a node name already taken by a non-table node', () => {
        const occupied: SchemaDefinition = {
            ...prosemirrorSchema,
            nodes: [
                ...prosemirrorSchema.nodes,
                { name: 'table', content: 'block+', group: 'block', role: 'block' },
            ],
        };

        expect(() => withTablesSchema(occupied)).toThrow(/table/);
    });
});

describe('table row content', () => {
    it('permits a row with zero cells', () => {
        const schema = withTablesSchema(prosemirrorSchema);
        const row = nodeNamed(schema, TABLE_NODE_NAMES.prosemirror.row);

        expect(minimalContentMatch<string>(row.content, () => undefined)).toEqual([]);
    });

    it('would reject zero cells for a one-or-more row expression', () => {
        const requiresOne = `(${TABLE_NODE_NAMES.prosemirror.cell} | ${TABLE_NODE_NAMES.prosemirror.headerCell})+`;

        expect(minimalContentMatch<string>(requiresOne, () => undefined)).toBeUndefined();
    });

    it('accepts both cell kinds in a row', () => {
        const schema = withTablesSchema(prosemirrorSchema);
        const row = nodeNamed(schema, TABLE_NODE_NAMES.prosemirror.row);

        expect(acceptingContentSymbols(row.content)).toEqual([
            TABLE_NODE_NAMES.prosemirror.cell,
            TABLE_NODE_NAMES.prosemirror.headerCell,
        ]);
    });

    it('requires at least one row in a table', () => {
        const schema = withTablesSchema(prosemirrorSchema);
        const table = nodeNamed(schema, TABLE_NODE_NAMES.prosemirror.table);

        expect(minimalContentMatch<string>(table.content, () => undefined)).toBeUndefined();
        expect(acceptingContentSymbols(table.content)).toEqual([
            TABLE_NODE_NAMES.prosemirror.row,
        ]);
    });
});

describe('table role resolution rejection', () => {
    const withNodes = (extra: NodeSpec[]): SchemaDefinition => ({
        ...prosemirrorSchema,
        nodes: [ ...prosemirrorSchema.nodes, ...extra ],
    });

    it('falls back to the default schema when a table role has no partners', () => {
        const schema = withNodes([
            {
                name: 'lonelyTable',
                content: 'block+',
                group: 'block',
                role: 'block',
                tableRole: 'table',
            },
        ]);

        expect(resolveDocumentSchema(schema)).toBe(defaultSchema);
    });

    it('falls back to the default schema when a table role is claimed twice', () => {
        const tabled = withTablesSchema(prosemirrorSchema);
        const duplicated: SchemaDefinition = {
            ...tabled,
            nodes: [
                ...tabled.nodes,
                {
                    name: 'secondCell',
                    content: 'block+',
                    role: 'block',
                    tableRole: 'cell',
                },
            ],
        };

        expect(resolveDocumentSchema(duplicated)).toBe(defaultSchema);
    });

    it('drops an unrecognized table role instead of failing the schema', () => {
        const schema = withNodes([
            {
                name: 'oddity',
                content: 'block+',
                group: 'block',
                role: 'block',
                tableRole: 'footer' as TableRole,
            },
        ]);
        const resolved = resolveDocumentSchema(schema);

        expect(resolved).not.toBe(defaultSchema);
        expect(nodeNamed(resolved, 'oddity').tableRole).toBeUndefined();
    });
});

describe('maxTableGridSlots configuration bounds', () => {
    it('exposes the documented default and hard ceiling', () => {
        expect(DEFAULT_MAX_TABLE_GRID_SLOTS).toBe(25_000);
        expect(HARD_MAX_TABLE_GRID_SLOTS).toBe(4_000_000);
        expect(resolveEditorResourceLimits().maxTableGridSlots).toBe(DEFAULT_MAX_TABLE_GRID_SLOTS);
    });

    it.each([ 0, HARD_MAX_TABLE_GRID_SLOTS + 1 ])('rejects %i grid slots', value => {
        const error = catchThrown(() =>
            resolveEditorResourceLimits({ maxTableGridSlots: value }));

        expect(error).toBeInstanceOf(NativeEditorBoundaryError);
        expect((error as NativeEditorBoundaryError).code).toBe('INVALID_RESOURCE_LIMIT');
        expect((error as NativeEditorBoundaryError).message).toContain(GRID_LIMIT_FIELD);
        expect((error as NativeEditorBoundaryError).limit).toBe(HARD_MAX_TABLE_GRID_SLOTS);
    });

    it.each([ DEFAULT_MAX_TABLE_GRID_SLOTS, HARD_MAX_TABLE_GRID_SLOTS ])(
        'accepts %i grid slots',
        value => {
            expect(resolveEditorResourceLimits({ maxTableGridSlots: value }).maxTableGridSlots).toBe(
                value
            );
        }
    );

    it('keeps node, byte, and depth limits independent of the grid limit', () => {
        const defaults = resolveEditorResourceLimits();
        const resolved = resolveEditorResourceLimits({
            maxTableGridSlots: HARD_MAX_TABLE_GRID_SLOTS,
        });

        expect(resolved.maxDocumentNodes).toBe(defaults.maxDocumentNodes);
        expect(resolved.maxDocumentDepth).toBe(defaults.maxDocumentDepth);
        expect(resolved.maxSchemaNodes).toBe(defaults.maxSchemaNodes);
        expect(resolved.maxInputBytes).toBe(defaults.maxInputBytes);
        expect(resolved.maxSchemaExpressionBytes).toBe(defaults.maxSchemaExpressionBytes);
    });
});
