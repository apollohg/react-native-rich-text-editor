import type { SuiteEvent } from './checkpoint-io.js';

export default async function* reporter(events: AsyncIterable<SuiteEvent>) {
    for await (const event of events)
        if (['test:pass', 'test:fail', 'test:summary'].includes(event.type))
            yield `${JSON.stringify(event, (_key, value: unknown) =>
                value instanceof Error
                    ? { message: value.message, stack: value.stack }
                    : value,
            )}\n`;
}
