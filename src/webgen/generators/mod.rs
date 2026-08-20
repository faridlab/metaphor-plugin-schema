//! Code generators for metaphor-webgen
//!
//! This module contains all specialized generators for producing TypeScript/React code.
//!
//! Layers:
//! - Domain: Entity types, schemas, value objects, repositories, services, events
//! - Application: Use cases, application services
//! - Presentation: Forms, tables, pages, detail views
//! - Infrastructure: API clients, repository implementations

pub mod application;
pub mod contracts;
pub mod domain;
pub mod infrastructure;
pub mod presentation;
pub mod shared_runtime;

// Domain layer re-exports
pub use domain::{
    CommandGenerator, DomainEventGenerator, DomainGenerationResult, DomainGenerator,
    DomainServiceGenerator, EntityGenerator, EntitySchemaGenerator, QueryGenerator,
    RepositoryGenerator, SpecificationGenerator, TypeMapper, ValueObjectGenerator,
};

// Contracts layer re-export (pure, framework-free genotype)
pub use contracts::ContractsGenerator;

// Application layer re-exports
pub use application::{AppServiceGenerator, ApplicationGenerator, UseCaseGenerator};

// Presentation layer re-exports
pub use presentation::{
    CrudPagesGenerator, DetailViewGenerator, FormFieldsGenerator, PresentationGenerator,
    TableColumnsGenerator,
};

// Infrastructure layer re-exports
pub use infrastructure::{
    ApiClientGenerator, GrpcClientGenerator, InfrastructureGenerator, RepositoryImplGenerator,
};
