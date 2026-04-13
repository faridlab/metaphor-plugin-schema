//! Code generators for metaphor-webgen
//!
//! This module contains all specialized generators for producing TypeScript/React code.
//!
//! Layers:
//! - Domain: Entity types, schemas, value objects, repositories, services, events
//! - Application: Use cases, application services
//! - Presentation: Forms, tables, pages, detail views
//! - Infrastructure: API clients, repository implementations

pub mod domain;
pub mod application;
pub mod presentation;
pub mod infrastructure;

// Domain layer re-exports
pub use domain::{
    DomainGenerator,
    DomainGenerationResult,
    EntityGenerator,
    EntitySchemaGenerator,
    ValueObjectGenerator,
    RepositoryGenerator,
    CommandGenerator,
    QueryGenerator,
    DomainServiceGenerator,
    DomainEventGenerator,
    SpecificationGenerator,
    TypeMapper,
};

// Application layer re-exports
pub use application::{
    ApplicationGenerator,
    UseCaseGenerator,
    AppServiceGenerator,
};

// Presentation layer re-exports
pub use presentation::{
    PresentationGenerator,
    FormFieldsGenerator,
    TableColumnsGenerator,
    CrudPagesGenerator,
    DetailViewGenerator,
};

// Infrastructure layer re-exports
pub use infrastructure::{
    InfrastructureGenerator,
    GrpcClientGenerator,
    ApiClientGenerator,
    RepositoryImplGenerator,
};
