//! Value Object generator for immutable value wrappers
//!
//! Generates TypeScript value object types with validation and equality.

use std::fs;
use std::collections::HashSet;

use crate::webgen::ast::entity::EntityDefinition;
use crate::webgen::config::Config;
use crate::webgen::error::Result;
use super::type_mapping::{TypeMapper, ValueObjectType};
use super::DomainGenerationResult;

/// Generator for value object types
pub struct ValueObjectGenerator {
    config: Config,
    type_mapper: TypeMapper,
}

impl ValueObjectGenerator {
    /// Create a new value object generator
    pub fn new(config: Config, type_mapper: TypeMapper) -> Self {
        Self { config, type_mapper }
    }

    /// Generate value objects from entity field analysis
    pub fn generate_from_entities(
        &self,
        entities: &[EntityDefinition],
    ) -> Result<DomainGenerationResult> {
        let mut result = DomainGenerationResult::new();

        // Collect unique value object types
        let mut vo_types: HashSet<ValueObjectType> = HashSet::new();

        for entity in entities {
            for field in &entity.fields {
                if let Some(vo_type) = self.type_mapper.detect_value_object_type(field) {
                    vo_types.insert(vo_type);
                }
            }
        }

        let vo_dir = self.config.output_dir
            .join("domain")
            .join(&self.config.module)
            .join("value_object");

        if !self.config.dry_run && !vo_types.is_empty() {
            fs::create_dir_all(&vo_dir).ok();
        }

        // Generate each value object type
        for vo_type in vo_types {
            let content = self.generate_value_object(&vo_type);
            let path = vo_dir.join(format!("{}.ts", vo_type.file_name()));

            result.add_file(path.clone(), self.config.dry_run);

            if !self.config.dry_run {
                fs::write(&path, content).ok();
            }
        }

        // Generate index file
        let index_content = self.generate_index();
        let index_path = vo_dir.join("index.ts");

        result.add_file(index_path.clone(), self.config.dry_run);

        if !self.config.dry_run {
            fs::create_dir_all(&vo_dir).ok();
            fs::write(&index_path, index_content).ok();
        }

        Ok(result)
    }

    /// Generate a single value object
    fn generate_value_object(&self, vo_type: &ValueObjectType) -> String {
        match vo_type {
            ValueObjectType::Email => self.generate_email_vo(),
            ValueObjectType::Phone => self.generate_phone_vo(),
            ValueObjectType::Address => self.generate_address_vo(),
            ValueObjectType::Money => self.generate_money_vo(),
            ValueObjectType::Url => self.generate_url_vo(),
            ValueObjectType::PersonName => self.generate_person_name_vo(),
            ValueObjectType::Identifier => self.generate_identifier_vo(),
            ValueObjectType::Custom(name) => self.generate_custom_vo(name),
        }
    }

    /// Generate Email value object
    fn generate_email_vo(&self) -> String {
        r#"/**
 * Email Value Object
 *
 * Immutable value object for email addresses with validation.
 */

import { z } from 'zod';

/**
 * Email schema with validation
 */
export const emailSchema = z.string().email().max(255);

/**
 * Email value object interface
 */
export interface Email {
  readonly value: string;
  readonly domain: string;
  readonly localPart: string;
}

/**
 * Create an Email value object
 */
export function createEmail(value: string): Email {
  const validated = emailSchema.parse(value.toLowerCase().trim());
  const [localPart, domain] = validated.split('@');

  return Object.freeze({
    value: validated,
    domain,
    localPart,
  });
}

/**
 * Check if a value is a valid Email
 */
export function isEmail(value: unknown): value is Email {
  return (
    typeof value === 'object' &&
    value !== null &&
    'value' in value &&
    'domain' in value &&
    'localPart' in value
  );
}

/**
 * Compare two Email objects for equality
 */
export function emailEquals(a: Email, b: Email): boolean {
  return a.value === b.value;
}

/**
 * Safely create an Email or return null
 */
export function tryCreateEmail(value: string): Email | null {
  const result = emailSchema.safeParse(value);
  if (result.success) {
    return createEmail(result.data);
  }
  return null;
}
"#.to_string()
    }

    /// Generate Phone value object
    fn generate_phone_vo(&self) -> String {
        r#"/**
 * PhoneNumber Value Object
 *
 * Immutable value object for phone numbers with formatting.
 */

import { z } from 'zod';

/**
 * Phone number schema
 */
export const phoneNumberSchema = z.string()
  .min(7)
  .max(20)
  .regex(/^[\d\s\-\+\(\)]+$/, 'Invalid phone number format');

/**
 * PhoneNumber value object interface
 */
export interface PhoneNumber {
  readonly value: string;
  readonly normalized: string;
  readonly countryCode?: string;
}

/**
 * Create a PhoneNumber value object
 */
export function createPhoneNumber(value: string, countryCode?: string): PhoneNumber {
  const validated = phoneNumberSchema.parse(value);
  const normalized = validated.replace(/[\s\-\(\)]/g, '');

  return Object.freeze({
    value: validated,
    normalized,
    countryCode,
  });
}

/**
 * Check if a value is a valid PhoneNumber
 */
export function isPhoneNumber(value: unknown): value is PhoneNumber {
  return (
    typeof value === 'object' &&
    value !== null &&
    'value' in value &&
    'normalized' in value
  );
}

/**
 * Compare two PhoneNumber objects for equality
 */
export function phoneNumberEquals(a: PhoneNumber, b: PhoneNumber): boolean {
  return a.normalized === b.normalized;
}

/**
 * Format phone number for display
 */
export function formatPhoneNumber(phone: PhoneNumber): string {
  // Basic formatting - can be enhanced with libphonenumber
  return phone.value;
}
"#.to_string()
    }

    /// Generate Address value object
    fn generate_address_vo(&self) -> String {
        r#"/**
 * Address Value Object
 *
 * Immutable value object for physical addresses.
 */

import { z } from 'zod';

/**
 * Address schema
 */
export const addressSchema = z.object({
  street1: z.string().min(1).max(255),
  street2: z.string().max(255).optional(),
  city: z.string().min(1).max(100),
  state: z.string().max(100).optional(),
  postalCode: z.string().max(20).optional(),
  country: z.string().min(2).max(100),
});

/**
 * Address value object interface
 */
export interface Address {
  readonly street1: string;
  readonly street2?: string;
  readonly city: string;
  readonly state?: string;
  readonly postalCode?: string;
  readonly country: string;
}

/**
 * Create an Address value object
 */
export function createAddress(data: z.infer<typeof addressSchema>): Address {
  const validated = addressSchema.parse(data);

  return Object.freeze({
    street1: validated.street1,
    street2: validated.street2,
    city: validated.city,
    state: validated.state,
    postalCode: validated.postalCode,
    country: validated.country,
  });
}

/**
 * Check if a value is a valid Address
 */
export function isAddress(value: unknown): value is Address {
  const result = addressSchema.safeParse(value);
  return result.success;
}

/**
 * Compare two Address objects for equality
 */
export function addressEquals(a: Address, b: Address): boolean {
  return (
    a.street1 === b.street1 &&
    a.street2 === b.street2 &&
    a.city === b.city &&
    a.state === b.state &&
    a.postalCode === b.postalCode &&
    a.country === b.country
  );
}

/**
 * Format address for display
 */
export function formatAddress(address: Address, options?: { multiline?: boolean }): string {
  const parts = [address.street1];

  if (address.street2) {
    parts.push(address.street2);
  }

  const cityLine = [address.city, address.state, address.postalCode]
    .filter(Boolean)
    .join(', ');
  parts.push(cityLine);
  parts.push(address.country);

  const separator = options?.multiline ? '\n' : ', ';
  return parts.join(separator);
}
"#.to_string()
    }

    /// Generate Money value object
    fn generate_money_vo(&self) -> String {
        r#"/**
 * Money Value Object
 *
 * Immutable value object for monetary amounts with currency.
 */

import { z } from 'zod';

/**
 * Currency code schema (ISO 4217)
 */
export const currencySchema = z.string().length(3).toUpperCase();

/**
 * Money schema
 */
export const moneySchema = z.object({
  amount: z.number().finite(),
  currency: currencySchema,
});

/**
 * Money value object interface
 */
export interface Money {
  readonly amount: number;
  readonly currency: string;
}

/**
 * Create a Money value object
 */
export function createMoney(amount: number, currency: string): Money {
  const validated = moneySchema.parse({ amount, currency });

  return Object.freeze({
    amount: validated.amount,
    currency: validated.currency,
  });
}

/**
 * Check if a value is a valid Money
 */
export function isMoney(value: unknown): value is Money {
  const result = moneySchema.safeParse(value);
  return result.success;
}

/**
 * Compare two Money objects for equality
 */
export function moneyEquals(a: Money, b: Money): boolean {
  return a.amount === b.amount && a.currency === b.currency;
}

/**
 * Add two Money values (must have same currency)
 */
export function addMoney(a: Money, b: Money): Money {
  if (a.currency !== b.currency) {
    throw new Error(`Cannot add money with different currencies: ${a.currency} and ${b.currency}`);
  }
  return createMoney(a.amount + b.amount, a.currency);
}

/**
 * Subtract two Money values (must have same currency)
 */
export function subtractMoney(a: Money, b: Money): Money {
  if (a.currency !== b.currency) {
    throw new Error(`Cannot subtract money with different currencies: ${a.currency} and ${b.currency}`);
  }
  return createMoney(a.amount - b.amount, a.currency);
}

/**
 * Multiply Money by a scalar
 */
export function multiplyMoney(money: Money, multiplier: number): Money {
  return createMoney(money.amount * multiplier, money.currency);
}

/**
 * Format money for display
 */
export function formatMoney(money: Money, locale = 'en-US'): string {
  return new Intl.NumberFormat(locale, {
    style: 'currency',
    currency: money.currency,
  }).format(money.amount);
}

/**
 * Zero money value
 */
export function zeroMoney(currency = 'USD'): Money {
  return createMoney(0, currency);
}
"#.to_string()
    }

    /// Generate URL value object
    fn generate_url_vo(&self) -> String {
        r#"/**
 * Url Value Object
 *
 * Immutable value object for URLs with parsing.
 */

import { z } from 'zod';

/**
 * URL schema
 */
export const urlSchema = z.string().url().max(2048);

/**
 * Url value object interface
 */
export interface Url {
  readonly value: string;
  readonly protocol: string;
  readonly host: string;
  readonly pathname: string;
  readonly search: string;
  readonly hash: string;
}

/**
 * Create a Url value object
 */
export function createUrl(value: string): Url {
  const validated = urlSchema.parse(value);
  const url = new URL(validated);

  return Object.freeze({
    value: validated,
    protocol: url.protocol,
    host: url.host,
    pathname: url.pathname,
    search: url.search,
    hash: url.hash,
  });
}

/**
 * Check if a value is a valid Url
 */
export function isUrl(value: unknown): value is Url {
  return (
    typeof value === 'object' &&
    value !== null &&
    'value' in value &&
    'protocol' in value &&
    'host' in value
  );
}

/**
 * Compare two Url objects for equality
 */
export function urlEquals(a: Url, b: Url): boolean {
  return a.value === b.value;
}

/**
 * Safely create a Url or return null
 */
export function tryCreateUrl(value: string): Url | null {
  const result = urlSchema.safeParse(value);
  if (result.success) {
    return createUrl(result.data);
  }
  return null;
}
"#.to_string()
    }

    /// Generate PersonName value object
    fn generate_person_name_vo(&self) -> String {
        r#"/**
 * PersonName Value Object
 *
 * Immutable value object for person names.
 */

import { z } from 'zod';

/**
 * PersonName schema
 */
export const personNameSchema = z.object({
  firstName: z.string().min(1).max(100),
  lastName: z.string().min(1).max(100),
  middleName: z.string().max(100).optional(),
  prefix: z.string().max(20).optional(),
  suffix: z.string().max(20).optional(),
});

/**
 * PersonName value object interface
 */
export interface PersonName {
  readonly firstName: string;
  readonly lastName: string;
  readonly middleName?: string;
  readonly prefix?: string;
  readonly suffix?: string;
}

/**
 * Create a PersonName value object
 */
export function createPersonName(data: z.infer<typeof personNameSchema>): PersonName {
  const validated = personNameSchema.parse(data);

  return Object.freeze({
    firstName: validated.firstName.trim(),
    lastName: validated.lastName.trim(),
    middleName: validated.middleName?.trim(),
    prefix: validated.prefix?.trim(),
    suffix: validated.suffix?.trim(),
  });
}

/**
 * Get full name string
 */
export function getFullName(name: PersonName): string {
  const parts = [];

  if (name.prefix) parts.push(name.prefix);
  parts.push(name.firstName);
  if (name.middleName) parts.push(name.middleName);
  parts.push(name.lastName);
  if (name.suffix) parts.push(name.suffix);

  return parts.join(' ');
}

/**
 * Get display name (First Last)
 */
export function getDisplayName(name: PersonName): string {
  return `${name.firstName} ${name.lastName}`;
}

/**
 * Compare two PersonName objects for equality
 */
export function personNameEquals(a: PersonName, b: PersonName): boolean {
  return (
    a.firstName === b.firstName &&
    a.lastName === b.lastName &&
    a.middleName === b.middleName
  );
}
"#.to_string()
    }

    /// Generate Identifier value object
    fn generate_identifier_vo(&self) -> String {
        r#"/**
 * Identifier Value Object
 *
 * Immutable value object for typed identifiers.
 */

import { z } from 'zod';

/**
 * Identifier schema
 */
export const identifierSchema = z.string().uuid();

/**
 * Identifier value object interface
 */
export interface Identifier<T extends string = string> {
  readonly value: string;
  readonly type: T;
}

/**
 * Create an Identifier value object
 */
export function createIdentifier<T extends string>(value: string, type: T): Identifier<T> {
  const validated = identifierSchema.parse(value);

  return Object.freeze({
    value: validated,
    type,
  });
}

/**
 * Generate a new random identifier
 */
export function generateIdentifier<T extends string>(type: T): Identifier<T> {
  return createIdentifier(crypto.randomUUID(), type);
}

/**
 * Compare two Identifier objects for equality
 */
export function identifierEquals<T extends string>(a: Identifier<T>, b: Identifier<T>): boolean {
  return a.value === b.value && a.type === b.type;
}

/**
 * Check if a value is a valid Identifier
 */
export function isIdentifier<T extends string>(value: unknown): value is Identifier<T> {
  return (
    typeof value === 'object' &&
    value !== null &&
    'value' in value &&
    'type' in value &&
    typeof (value as Identifier<T>).value === 'string' &&
    typeof (value as Identifier<T>).type === 'string'
  );
}
"#.to_string()
    }

    /// Generate a custom value object template
    fn generate_custom_vo(&self, name: &str) -> String {
        format!(
r#"/**
 * {name} Value Object
 *
 * Custom value object - implement according to domain requirements.
 */

import {{ z }} from 'zod';

/**
 * {name} schema
 */
export const {name_lower}Schema = z.object({{
  // Add fields here
}});

/**
 * {name} value object interface
 */
export interface {name} {{
  // Add properties here
}}

/**
 * Create a {name} value object
 */
export function create{name}(data: z.infer<typeof {name_lower}Schema>): {name} {{
  const validated = {name_lower}Schema.parse(data);

  return Object.freeze({{
    ...validated,
  }});
}}

/**
 * Compare two {name} objects for equality
 */
export function {name_lower}Equals(a: {name}, b: {name}): boolean {{
  // Implement equality check
  return JSON.stringify(a) === JSON.stringify(b);
}}
"#,
            name = name,
            name_lower = name.to_lowercase(),
        )
    }

    /// Generate value object index file
    fn generate_index(&self) -> String {
        r#"// Value Object exports - Generated by metaphor-webgen
// Do not edit manually

export * from './Email';
export * from './PhoneNumber';
export * from './Address';
export * from './Money';
export * from './Url';
export * from './PersonName';
export * from './Identifier';

// <<< CUSTOM: Add custom value object exports here
// END CUSTOM
"#.to_string()
    }
}

