import {
    defaultEmptyDocument,
    defaultSchema,
    defineSchema,
    resolveDocumentSchema,
    TASK_ITEM_NODE_NAME,
    TASK_LIST_NODE_NAME,
    tiptapCompatibleSchema,
    tiptapCompatibleSchemaSpec,
    withTaskListSchema,
} from '../schemas';

describe('task list schemas', () => {
    it('retains Tiptap HTML discriminators and checked mapping in resolved schemas', () => {
        for (const schema of [tiptapCompatibleSchema, withTaskListSchema(defaultSchema)]) {
            const nodes = resolveDocumentSchema(schema).nodes;
            expect(nodes.find((node) => node.name === TASK_LIST_NODE_NAME)?.html).toEqual({
                tag: 'ul',
                staticAttrs: { 'data-type': TASK_LIST_NODE_NAME },
            });
            expect(nodes.find((node) => node.name === TASK_ITEM_NODE_NAME)?.html).toEqual({
                tag: 'li',
                staticAttrs: { 'data-type': TASK_ITEM_NODE_NAME },
                attrMap: { checked: 'data-checked' },
            });
        }
    });

    it('constructs task items with unchecked defaults through the Tiptap preset', () => {
        const schema = defineSchema({
            ...tiptapCompatibleSchemaSpec,
            nodes: {
                ...tiptapCompatibleSchemaSpec.nodes,
                doc: { content: 'taskList+', role: 'doc' },
            },
        });

        expect(defaultEmptyDocument(schema)).toEqual({
            type: 'doc',
            content: [
                {
                    type: 'taskList',
                    content: [
                        {
                            type: 'taskItem',
                            attrs: { checked: false },
                            content: [{ type: 'paragraph', attrs: { textAlign: null } }],
                        },
                    ],
                },
            ],
        });
    });

    it('adds task lists to the default schema without mutating it', () => {
        const originalNodes = [...defaultSchema.nodes];
        const schema = withTaskListSchema(defaultSchema);

        expect(defaultSchema.nodes).toEqual(originalNodes);
        expect(resolveDocumentSchema(schema).nodes).toEqual(
            expect.arrayContaining([
                expect.objectContaining({
                    name: TASK_LIST_NODE_NAME,
                    content: `${TASK_ITEM_NODE_NAME}+`,
                    role: 'list',
                    group: 'block',
                }),
                expect.objectContaining({
                    name: TASK_ITEM_NODE_NAME,
                    content: 'paragraph block*',
                    role: 'listItem',
                    attrs: { checked: { type: 'boolean', default: false } },
                }),
            ])
        );
        expect(withTaskListSchema(schema)).toBe(schema);
        expect(withTaskListSchema(tiptapCompatibleSchema)).toBe(tiptapCompatibleSchema);
    });

    it.each(['taskList', 'taskItem'])(
        'preserves an existing %s declaration while adding its missing partner',
        (existingName) => {
            const existingNode = {
                ...tiptapCompatibleSchema.nodes.find((node) => node.name === existingName)!,
                attrs: { checked: { default: true }, custom: { default: 'retained' } },
            };
            const schema = withTaskListSchema({
                ...defaultSchema,
                nodes: [...defaultSchema.nodes, existingNode],
            });

            expect(schema.nodes.filter((node) => node.name === existingName)).toEqual([
                existingNode,
            ]);
            expect(schema.nodes.find((node) => node.name === existingName)).toBe(existingNode);
            expect(schema.nodes.some((node) => node.name === 'taskList')).toBe(true);
            expect(schema.nodes.some((node) => node.name === 'taskItem')).toBe(true);
        }
    );
});
