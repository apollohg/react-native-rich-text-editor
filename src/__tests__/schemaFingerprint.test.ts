import fixtures from '../../rust/editor-core/tests/fixtures/schema-fingerprints.json';
import {
    TABLE_NODE_NAMES,
    defaultSchema,
    prosemirrorSchema,
    tiptapCompatibleSchema,
    withTablesSchema,
    type SchemaDefinition,
} from '../schemas';
import { testSchemaFingerprint } from './helpers/schemaFingerprint';

describe('resolved schema fingerprint parity', () => {
    it('matches the Tiptap-compatible schema to the native fixture', () => {
        const fixture = fixtures.fingerprints.find(
            ({ name }) => name === 'Tiptap-compatible camelCase schema'
        )!;

        expect(testSchemaFingerprint(tiptapCompatibleSchema)).toBe(fixture.expectedFingerprint);
    });

    it('matches the exported ProseMirror schema to the native fixture', () => {
        const fixture = fixtures.fingerprints.find(
            ({ name }) => name === 'default ProseMirror schema'
        )!;

        expect(testSchemaFingerprint(prosemirrorSchema)).toBe(fixture.expectedFingerprint);
        expect(defaultSchema).toBe(prosemirrorSchema);
    });

    it.each(fixtures.fingerprints)('$name matches the checked-in Rust fingerprint', fixture => {
        expect(testSchemaFingerprint(fixture.schema as SchemaDefinition)).toBe(
            fixture.expectedFingerprint
        );
    });

    it.each(fixtures.equivalentSchemas)('$name ignores object key insertion order', fixture => {
        for (const schema of fixture.schemas) {
            expect(testSchemaFingerprint(schema as SchemaDefinition)).toBe(
                fixture.expectedFingerprint
            );
        }
    });

    it('includes JSON projections in the canonical fingerprint', () => {
        const schema = (tone: string): SchemaDefinition => ({
            nodes: [
                { name: 'doc', content: 'infoBox', role: 'doc' },
                {
                    name: 'infoBox',
                    content: '',
                    role: 'block',
                    json: { type: 'callout', attrs: { tone } },
                },
                { name: 'text', content: '', role: 'text' },
            ],
            marks: [],
        });

        expect(testSchemaFingerprint(schema('info'))).not.toBe(
            testSchemaFingerprint(schema('warning'))
        );
    });

    it('includes table roles in the canonical fingerprint', () => {
        const tabled = withTablesSchema(prosemirrorSchema);
        const swapped: SchemaDefinition = {
            ...tabled,
            nodes: tabled.nodes.map(node => {
                if (node.name === TABLE_NODE_NAMES.prosemirror.cell) {
                    return { ...node, tableRole: 'header_cell' as const };
                }

                if (node.name === TABLE_NODE_NAMES.prosemirror.headerCell) {
                    return { ...node, tableRole: 'cell' as const };
                }

                return node;
            }),
        };

        expect(testSchemaFingerprint(tabled)).not.toBe(testSchemaFingerprint(prosemirrorSchema));
        expect(testSchemaFingerprint(swapped)).not.toBe(testSchemaFingerprint(tabled));
    });

    it('keeps the helper test-only', () => {
        expect(require.resolve('./helpers/schemaFingerprint')).toContain('/src/__tests__/helpers/');
    });
});
