import { createHash } from 'node:crypto';
import type { UpdateEvent } from './peer-protocol.js';
import { assertDrainBound } from './assertions.js';

export interface ScheduledMessage {
    id: string;
    sender: number;
    recipient: number;
    sequence: number;
    event: UpdateEvent;
}

export interface DeliveryRecord {
    id: string;
    attempt: number;
    failed: boolean;
    sender: number;
    recipient: number;
    sequence: number;
    origin: UpdateEvent['origin'];
    byteLength: number;
    digest: string;
    round: number;
    deferredRounds: number;
}

export interface RoundRecord {
    round: number;
    attempted: string[];
    delivered: string[];
    deferred: string[];
    emitted: number;
    dependencyWaits: number[];
}

export interface RoundFlush {
    emitted: number;
    dependencyWaits: number[];
}

export interface FlushResult {
    events: UpdateEvent[];
    pendingDependencies: boolean;
}

export interface SchedulerAccess {
    peerCount(): number;
    deliver(recipient: number, updateBase64: string): Promise<void>;
    flush(peer: number): Promise<FlushResult>;
}

export interface DeliveryObserver {
    onDelivery(record: DeliveryRecord, message: ScheduledMessage): void;
}

export interface SchedulerOptions {
    seed: number;
    observer?: DeliveryObserver;
}

const DELIVERY_ID_PREFIX = 'd';
const DIGEST_ALGORITHM = 'sha256';
const DIGEST_LENGTH = 16;
const MAX_TRACE_DELIVERIES = 24;

export function nextRandom(seed: number): number {
    let value = seed | 0;
    value ^= value << 13;
    value ^= value >>> 17;
    value ^= value << 5;
    return value >>> 0;
}

function digestOf(bytesBase64: string): string {
    return createHash(DIGEST_ALGORITHM)
        .update(Buffer.from(bytesBase64, 'base64'))
        .digest('hex')
        .slice(0, DIGEST_LENGTH);
}

function linkKey(one: number, other: number): string {
    return one < other ? `${one}:${other}` : `${other}:${one}`;
}

export class DeliveryScheduler {
    private readonly access: SchedulerAccess;
    private readonly observer: DeliveryObserver | null;
    private readonly initialSeed: number;
    private randomState: number;
    private readonly queue: ScheduledMessage[] = [];
    private readonly isolatedLinks = new Set<string>();
    private readonly deliveryRecords: DeliveryRecord[] = [];
    private readonly roundRecords: RoundRecord[] = [];
    private readonly sequences = new Map<number, number>();
    private readonly deferrals = new Map<string, number>();
    private readonly droppedIds: string[] = [];
    private nextDeliveryNumber = 0;

    constructor(access: SchedulerAccess, options: SchedulerOptions) {
        this.access = access;
        this.observer = options.observer ?? null;
        this.initialSeed = options.seed;
        this.randomState = options.seed;
    }

    pending(): readonly ScheduledMessage[] {
        return this.queue;
    }

    deliveries(): readonly DeliveryRecord[] {
        return this.deliveryRecords;
    }

    drainStats(): { rounds: number; emitted: number } {
        return {
            rounds: this.roundRecords.length,
            emitted: this.roundRecords.reduce((total, round) => total + round.emitted, 0),
        };
    }

    enqueue(sender: number, event: UpdateEvent): ScheduledMessage[] {
        if (event.kind !== 'document') {
            throw new Error(`TBL-21 SCHEDULER_MISUSE: ${event.kind} events are not document deliveries`);
        }
        const sequence = (this.sequences.get(sender) ?? 0) + 1;
        this.sequences.set(sender, sequence);
        const created: ScheduledMessage[] = [];
        for (let recipient = 0; recipient < this.access.peerCount(); recipient += 1) {
            if (recipient === sender) {
                continue;
            }
            const message: ScheduledMessage = {
                id: this.mintDeliveryId(),
                sender,
                recipient,
                sequence,
                event,
            };
            this.queue.push(message);
            created.push(message);
        }
        return created;
    }

    duplicate(messageId: string): ScheduledMessage {
        const original = this.queue.find((message) => message.id === messageId);
        if (original === undefined) {
            throw new Error(`TBL-21 SCHEDULER_MISUSE: no pending message ${messageId} to duplicate`);
        }
        const copy: ScheduledMessage = { ...original, id: this.mintDeliveryId() };
        this.queue.push(copy);
        return copy;
    }

    drop(messageId: string): ScheduledMessage {
        const index = this.queue.findIndex((message) => message.id === messageId);
        const message = this.queue[index];
        if (index === -1 || message === undefined) {
            throw new Error(`TBL-21 SCHEDULER_MISUSE: no pending message ${messageId} to drop`);
        }
        this.queue.splice(index, 1);
        this.droppedIds.push(messageId);
        return message;
    }

    partition(one: number, other: number, isolated: boolean): void {
        if (one === other) {
            throw new Error('TBL-21 SCHEDULER_MISUSE: a peer cannot be partitioned from itself');
        }
        const key = linkKey(one, other);
        if (isolated) {
            this.isolatedLinks.add(key);
            return;
        }
        this.isolatedLinks.delete(key);
    }

    isPartitioned(one: number, other: number): boolean {
        return this.isolatedLinks.has(linkKey(one, other));
    }

    async deliver(message: ScheduledMessage): Promise<void> {
        if (this.isPartitioned(message.sender, message.recipient)) {
            throw new Error(
                `TBL-21 PARTITIONED: ${message.id} cannot cross the isolated link ${message.sender}-${message.recipient}`,
            );
        }
        const index = this.queue.findIndex((queued) => queued.id === message.id);
        if (index !== -1) {
            this.queue.splice(index, 1);
        }
        const record = this.record(message, this.roundRecords.length);
        try {
            await this.access.deliver(message.recipient, message.event.bytesBase64);
        } catch (error) {
            record.failed = true;
            if (index !== -1) {
                this.queue.splice(index, 0, message);
            }
            this.observer?.onDelivery(record, message);
            throw error;
        }
        this.observer?.onDelivery(record, message);
    }

    async collect(): Promise<RoundFlush> {
        const flush: RoundFlush = { emitted: 0, dependencyWaits: [] };
        for (let peer = 0; peer < this.access.peerCount(); peer += 1) {
            const flushed = await this.access.flush(peer);
            if (flushed.pendingDependencies) {
                flush.dependencyWaits.push(peer);
            }
            for (const event of flushed.events) {
                if (event.kind !== 'document') {
                    continue;
                }
                this.enqueue(peer, event);
                flush.emitted += 1;
            }
        }
        return flush;
    }

    async drain(): Promise<void> {
        let emittedTotal = 0;
        for (let round = 1; round <= Number.MAX_SAFE_INTEGER; round += 1) {
            const attempts = this.orderedAttempts();
            const record: RoundRecord = {
                round,
                attempted: attempts.map((message) => message.id),
                delivered: [],
                deferred: [],
                emitted: 0,
                dependencyWaits: [],
            };
            this.roundRecords.push(record);
            for (const message of attempts) {
                if (this.isPartitioned(message.sender, message.recipient)) {
                    record.deferred.push(message.id);
                    this.deferrals.set(message.id, (this.deferrals.get(message.id) ?? 0) + 1);
                    continue;
                }
                await this.deliver(message);
                record.delivered.push(message.id);
            }
            const flush = await this.collect();
            record.emitted = flush.emitted;
            record.dependencyWaits.push(...flush.dependencyWaits);
            emittedTotal += record.emitted;
            this.assertBounded(round, emittedTotal);
            if (this.queue.length === 0 && record.emitted === 0) {
                if (record.dependencyWaits.length === 0) {
                    return;
                }
                throw new Error(
                    `TBL-21 UNRESOLVED_DEPENDENCIES: peers ${record.dependencyWaits.join(',')} still hold quarantined updates with an empty queue; ${this.traceSummary()}`,
                );
            }
            if (record.delivered.length === 0 && record.emitted === 0 && record.deferred.length > 0) {
                throw new Error(
                    `TBL-21 PARTITIONED: ${record.deferred.length} messages cannot cross isolated links; ${this.traceSummary()}`,
                );
            }
        }
    }

    traceSummary(): string {
        const deliveries = this.deliveryRecords.slice(-MAX_TRACE_DELIVERIES);
        return JSON.stringify({
            seed: this.initialSeed,
            dropped: this.droppedIds,
            rounds: this.roundRecords.slice(-MAX_TRACE_DELIVERIES),
            deliveries,
            pending: this.queue.map((message) => ({
                id: message.id,
                sender: message.sender,
                recipient: message.recipient,
                sequence: message.sequence,
                digest: digestOf(message.event.bytesBase64),
            })),
        });
    }

    private assertBounded(round: number, emittedTotal: number): void {
        try {
            assertDrainBound(round, emittedTotal);
        } catch (error) {
            const reason = error instanceof Error ? error.message : String(error);
            throw new Error(`${reason}: ${round} rounds, ${emittedTotal} updates; ${this.traceSummary()}`);
        }
    }

    private mintDeliveryId(): string {
        this.nextDeliveryNumber += 1;
        return `${DELIVERY_ID_PREFIX}${this.nextDeliveryNumber}`;
    }

    private record(message: ScheduledMessage, round: number): DeliveryRecord {
        const record: DeliveryRecord = {
            id: message.id,
            attempt: this.deliveryRecords.filter((entry) => entry.id === message.id).length + 1,
            failed: false,
            sender: message.sender,
            recipient: message.recipient,
            sequence: message.sequence,
            origin: message.event.origin,
            byteLength: Buffer.from(message.event.bytesBase64, 'base64').length,
            digest: digestOf(message.event.bytesBase64),
            round,
            deferredRounds: this.deferrals.get(message.id) ?? 0,
        };
        this.deliveryRecords.push(record);
        return record;
    }

    private orderedAttempts(): ScheduledMessage[] {
        const attempts = [...this.queue];
        for (let index = attempts.length - 1; index > 0; index -= 1) {
            this.randomState = nextRandom(this.randomState);
            const target = this.randomState % (index + 1);
            const held = attempts[index];
            const swapped = attempts[target];
            if (held === undefined || swapped === undefined) {
                throw new Error('TBL-21 SCHEDULER_MISUSE: the attempt permutation left a hole');
            }
            attempts[index] = swapped;
            attempts[target] = held;
        }
        return attempts;
    }
}
