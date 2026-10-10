import './helpers/NativeRichTextEditorFixture';
import fs from 'node:fs';
import path from 'node:path';

import { TABLE_TOOLBAR_ACTIONS } from '../useTableToolbar';

const PARITY_FIXTURE_PATH = path.resolve(__dirname, '../../scripts/tests/table-toolbar-actions.json');

describe('native table accessibility action parity fixture', () => {
    it('lists exactly the toolbar actions in toolbar order', () => {
        const fixture: unknown = JSON.parse(fs.readFileSync(PARITY_FIXTURE_PATH, 'utf8'));
        const toolbar = Object.entries(TABLE_TOOLBAR_ACTIONS).map(([ action, spec ]) => ({
            action,
            applicability: spec.applicability,
            command: spec.command,
        }));

        expect(fixture).toEqual(toolbar);
    });
});
