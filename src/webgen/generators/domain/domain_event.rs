//! Domain Event generator for TypeScript domain layer
//!
//! Generates event types for domain events (Created, Updated, Deleted, etc.).

use std::fs;

use crate::webgen::ast::entity::EntityDefinition;
use crate::webgen::config::Config;
use crate::webgen::error::Result;
use crate::webgen::parser::{to_pascal_case, to_camel_case, to_snake_case};
use super::DomainGenerationResult;

/// Generator for domain event types
pub struct DomainEventGenerator {
    config: Config,
}

impl DomainEventGenerator {
    /// Create a new domain event generator
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    /// Generate domain events for an entity
    pub fn generate(&self, entity: &EntityDefinition) -> Result<DomainGenerationResult> {
        let mut result = DomainGenerationResult::new();

        let entity_pascal = to_pascal_case(&entity.name);
        let events_dir = self.config.output_dir
            .join("domain")
            .join(&self.config.module)
            .join("event");

        if !self.config.dry_run {
            fs::create_dir_all(&events_dir).ok();
        }

        let content = self.generate_events_content(entity);
        let path = events_dir.join(format!("{}Events.ts", entity_pascal));

        result.add_file(path.clone(), self.config.dry_run);

        if !self.config.dry_run {
            fs::write(&path, content).ok();
        }

        Ok(result)
    }

    /// Generate domain events content
    fn generate_events_content(&self, entity: &EntityDefinition) -> String {
        let entity_pascal = to_pascal_case(&entity.name);
        let entity_upper = entity.name.to_uppercase();
        let entity_snake = to_snake_case(&entity.name);

        format!(
r#"/**
 * {entity_pascal} Domain Events
 *
 * Event types for {entity_pascal} lifecycle and business operations.
 * These events follow the Event Sourcing pattern and can be used for:
 * - Audit logging
 * - Real-time notifications
 * - Event-driven integrations
 * - Undo/Redo functionality
 *
 * @module {module}/event/{entity_pascal}Events
 */

import {{ z }} from 'zod';
import type {{ {entity_pascal} }} from '../entity/{entity_pascal}.schema';

// ============================================================================
// Event Types
// ============================================================================

/**
 * Base event metadata
 */
export interface EventMetadata {{
  eventId: string;
  timestamp: Date;
  correlationId?: string;
  causationId?: string;
  userId?: string;
  version: number;
}}

/**
 * Base domain event interface
 */
export interface DomainEvent<T extends string, P> {{
  type: T;
  aggregateId: string;
  aggregateType: '{entity_pascal}';
  payload: P;
  metadata: EventMetadata;
}}

// ============================================================================
// {entity_pascal} Event Types
// ============================================================================

/**
 * Event type constants
 */
export const {entity_upper}_EVENTS = {{
  CREATED: '{entity_snake}.created',
  UPDATED: '{entity_snake}.updated',
  DELETED: '{entity_snake}.deleted',
  RESTORED: '{entity_snake}.restored',
  STATUS_CHANGED: '{entity_snake}.status_changed',
  ARCHIVED: '{entity_snake}.archived',
}} as const;

export type {entity_pascal}EventType = typeof {entity_upper}_EVENTS[keyof typeof {entity_upper}_EVENTS];

// ============================================================================
// Created Event
// ============================================================================

/**
 * {entity_pascal}Created event payload
 */
export interface {entity_pascal}CreatedPayload {{
  {entity_camel}: {entity_pascal};
}}

/**
 * {entity_pascal}Created event
 */
export type {entity_pascal}CreatedEvent = DomainEvent<
  typeof {entity_upper}_EVENTS.CREATED,
  {entity_pascal}CreatedPayload
>;

/**
 * Create a {entity_pascal}Created event
 */
export function {entity_camel}Created(
  {entity_camel}: {entity_pascal},
  metadata: Partial<EventMetadata> = {{}}
): {entity_pascal}CreatedEvent {{
  return {{
    type: {entity_upper}_EVENTS.CREATED,
    aggregateId: {entity_camel}.id,
    aggregateType: '{entity_pascal}',
    payload: {{ {entity_camel} }},
    metadata: createEventMetadata(metadata),
  }};
}}

// ============================================================================
// Updated Event
// ============================================================================

/**
 * {entity_pascal}Updated event payload
 */
export interface {entity_pascal}UpdatedPayload {{
  previous: Partial<{entity_pascal}>;
  current: {entity_pascal};
  changedFields: string[];
}}

/**
 * {entity_pascal}Updated event
 */
export type {entity_pascal}UpdatedEvent = DomainEvent<
  typeof {entity_upper}_EVENTS.UPDATED,
  {entity_pascal}UpdatedPayload
>;

/**
 * Create a {entity_pascal}Updated event
 */
export function {entity_camel}Updated(
  previous: Partial<{entity_pascal}>,
  current: {entity_pascal},
  changedFields: string[],
  metadata: Partial<EventMetadata> = {{}}
): {entity_pascal}UpdatedEvent {{
  return {{
    type: {entity_upper}_EVENTS.UPDATED,
    aggregateId: current.id,
    aggregateType: '{entity_pascal}',
    payload: {{ previous, current, changedFields }},
    metadata: createEventMetadata(metadata),
  }};
}}

// ============================================================================
// Deleted Event
// ============================================================================

/**
 * {entity_pascal}Deleted event payload
 */
export interface {entity_pascal}DeletedPayload {{
  id: string;
  softDelete: boolean;
  reason?: string;
}}

/**
 * {entity_pascal}Deleted event
 */
export type {entity_pascal}DeletedEvent = DomainEvent<
  typeof {entity_upper}_EVENTS.DELETED,
  {entity_pascal}DeletedPayload
>;

/**
 * Create a {entity_pascal}Deleted event
 */
export function {entity_camel}Deleted(
  id: string,
  softDelete: boolean = true,
  reason?: string,
  metadata: Partial<EventMetadata> = {{}}
): {entity_pascal}DeletedEvent {{
  return {{
    type: {entity_upper}_EVENTS.DELETED,
    aggregateId: id,
    aggregateType: '{entity_pascal}',
    payload: {{ id, softDelete, reason }},
    metadata: createEventMetadata(metadata),
  }};
}}

// ============================================================================
// Restored Event
// ============================================================================

/**
 * {entity_pascal}Restored event payload
 */
export interface {entity_pascal}RestoredPayload {{
  {entity_camel}: {entity_pascal};
}}

/**
 * {entity_pascal}Restored event
 */
export type {entity_pascal}RestoredEvent = DomainEvent<
  typeof {entity_upper}_EVENTS.RESTORED,
  {entity_pascal}RestoredPayload
>;

/**
 * Create a {entity_pascal}Restored event
 */
export function {entity_camel}Restored(
  {entity_camel}: {entity_pascal},
  metadata: Partial<EventMetadata> = {{}}
): {entity_pascal}RestoredEvent {{
  return {{
    type: {entity_upper}_EVENTS.RESTORED,
    aggregateId: {entity_camel}.id,
    aggregateType: '{entity_pascal}',
    payload: {{ {entity_camel} }},
    metadata: createEventMetadata(metadata),
  }};
}}

// ============================================================================
// Status Changed Event
// ============================================================================

/**
 * {entity_pascal}StatusChanged event payload
 */
export interface {entity_pascal}StatusChangedPayload {{
  id: string;
  previousStatus: string;
  newStatus: string;
  reason?: string;
}}

/**
 * {entity_pascal}StatusChanged event
 */
export type {entity_pascal}StatusChangedEvent = DomainEvent<
  typeof {entity_upper}_EVENTS.STATUS_CHANGED,
  {entity_pascal}StatusChangedPayload
>;

/**
 * Create a {entity_pascal}StatusChanged event
 */
export function {entity_camel}StatusChanged(
  id: string,
  previousStatus: string,
  newStatus: string,
  reason?: string,
  metadata: Partial<EventMetadata> = {{}}
): {entity_pascal}StatusChangedEvent {{
  return {{
    type: {entity_upper}_EVENTS.STATUS_CHANGED,
    aggregateId: id,
    aggregateType: '{entity_pascal}',
    payload: {{ id, previousStatus, newStatus, reason }},
    metadata: createEventMetadata(metadata),
  }};
}}

// ============================================================================
// Archived Event
// ============================================================================

/**
 * {entity_pascal}Archived event payload
 */
export interface {entity_pascal}ArchivedPayload {{
  id: string;
  archivedAt: Date;
  reason?: string;
}}

/**
 * {entity_pascal}Archived event
 */
export type {entity_pascal}ArchivedEvent = DomainEvent<
  typeof {entity_upper}_EVENTS.ARCHIVED,
  {entity_pascal}ArchivedPayload
>;

/**
 * Create a {entity_pascal}Archived event
 */
export function {entity_camel}Archived(
  id: string,
  reason?: string,
  metadata: Partial<EventMetadata> = {{}}
): {entity_pascal}ArchivedEvent {{
  return {{
    type: {entity_upper}_EVENTS.ARCHIVED,
    aggregateId: id,
    aggregateType: '{entity_pascal}',
    payload: {{ id, archivedAt: new Date(), reason }},
    metadata: createEventMetadata(metadata),
  }};
}}

// ============================================================================
// Union Types
// ============================================================================

/**
 * All {entity_pascal} domain events
 */
export type {entity_pascal}Event =
  | {entity_pascal}CreatedEvent
  | {entity_pascal}UpdatedEvent
  | {entity_pascal}DeletedEvent
  | {entity_pascal}RestoredEvent
  | {entity_pascal}StatusChangedEvent
  | {entity_pascal}ArchivedEvent;

// ============================================================================
// Event Utilities
// ============================================================================

/**
 * Create event metadata with defaults
 */
function createEventMetadata(partial: Partial<EventMetadata> = {{}}): EventMetadata {{
  return {{
    eventId: partial.eventId ?? crypto.randomUUID(),
    timestamp: partial.timestamp ?? new Date(),
    correlationId: partial.correlationId,
    causationId: partial.causationId,
    userId: partial.userId,
    version: partial.version ?? 1,
  }};
}}

/**
 * Type guard for {entity_pascal} events
 */
export function is{entity_pascal}Event(event: unknown): event is {entity_pascal}Event {{
  if (typeof event !== 'object' || event === null) return false;
  const e = event as {entity_pascal}Event;
  return (
    e.aggregateType === '{entity_pascal}' &&
    Object.values({entity_upper}_EVENTS).includes(e.type as typeof {entity_upper}_EVENTS[keyof typeof {entity_upper}_EVENTS])
  );
}}

/**
 * Type guard for specific event type
 */
export function is{entity_pascal}CreatedEvent(event: unknown): event is {entity_pascal}CreatedEvent {{
  return is{entity_pascal}Event(event) && event.type === {entity_upper}_EVENTS.CREATED;
}}

export function is{entity_pascal}UpdatedEvent(event: unknown): event is {entity_pascal}UpdatedEvent {{
  return is{entity_pascal}Event(event) && event.type === {entity_upper}_EVENTS.UPDATED;
}}

export function is{entity_pascal}DeletedEvent(event: unknown): event is {entity_pascal}DeletedEvent {{
  return is{entity_pascal}Event(event) && event.type === {entity_upper}_EVENTS.DELETED;
}}

// ============================================================================
// Event Handler Types
// ============================================================================

/**
 * Event handler function type
 */
export type {entity_pascal}EventHandler<E extends {entity_pascal}Event> = (
  event: E
) => void | Promise<void>;

/**
 * Event handlers registry
 */
export interface {entity_pascal}EventHandlers {{
  onCreated?: {entity_pascal}EventHandler<{entity_pascal}CreatedEvent>;
  onUpdated?: {entity_pascal}EventHandler<{entity_pascal}UpdatedEvent>;
  onDeleted?: {entity_pascal}EventHandler<{entity_pascal}DeletedEvent>;
  onRestored?: {entity_pascal}EventHandler<{entity_pascal}RestoredEvent>;
  onStatusChanged?: {entity_pascal}EventHandler<{entity_pascal}StatusChangedEvent>;
  onArchived?: {entity_pascal}EventHandler<{entity_pascal}ArchivedEvent>;
}}

/**
 * Dispatch event to handlers
 */
export async function dispatch{entity_pascal}Event(
  event: {entity_pascal}Event,
  handlers: {entity_pascal}EventHandlers
): Promise<void> {{
  switch (event.type) {{
    case {entity_upper}_EVENTS.CREATED:
      await handlers.onCreated?.(event);
      break;
    case {entity_upper}_EVENTS.UPDATED:
      await handlers.onUpdated?.(event);
      break;
    case {entity_upper}_EVENTS.DELETED:
      await handlers.onDeleted?.(event);
      break;
    case {entity_upper}_EVENTS.RESTORED:
      await handlers.onRestored?.(event);
      break;
    case {entity_upper}_EVENTS.STATUS_CHANGED:
      await handlers.onStatusChanged?.(event);
      break;
    case {entity_upper}_EVENTS.ARCHIVED:
      await handlers.onArchived?.(event);
      break;
  }}
}}

// <<< CUSTOM: Add custom event types here
// END CUSTOM
"#,
            entity_pascal = entity_pascal,
            entity_camel = to_camel_case(&entity.name),
            entity_upper = entity_upper,
            entity_snake = entity_snake,
            module = self.config.module,
        )
    }
}
