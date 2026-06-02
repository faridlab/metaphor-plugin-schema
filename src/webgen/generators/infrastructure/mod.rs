//! Infrastructure layer generators
//!
//! Generates:
//! - gRPC client implementations
//! - API client implementations (REST fallback)
//! - Repository implementations
//! - Service initialization
//! - External service adapters

mod grpc_client;
mod api_client;
mod repository_impl;
mod service_init;

pub use grpc_client::GrpcClientGenerator;
pub use api_client::ApiClientGenerator;
pub use repository_impl::RepositoryImplGenerator;
pub use service_init::ServiceInitGenerator;

use std::fs;

use crate::webgen::ast::entity::{EntityDefinition, EnumDefinition};
use crate::webgen::config::Config;
use crate::webgen::error::Result;
use crate::webgen::generators::domain::DomainGenerationResult;
use crate::webgen::parser::to_pascal_case;

/// Infrastructure layer generator
pub struct InfrastructureGenerator {
    grpc_client_gen: GrpcClientGenerator,
    api_client_gen: ApiClientGenerator,
    repository_impl_gen: RepositoryImplGenerator,
    service_init_gen: ServiceInitGenerator,
    config: Config,
}

impl InfrastructureGenerator {
    /// Create a new infrastructure generator
    pub fn new(config: Config) -> Self {
        Self {
            grpc_client_gen: GrpcClientGenerator::new(config.clone()),
            api_client_gen: ApiClientGenerator::new(config.clone()),
            repository_impl_gen: RepositoryImplGenerator::new(config.clone()),
            service_init_gen: ServiceInitGenerator::new(config.clone()),
            config,
        }
    }

    /// Generate all infrastructure layer components
    pub fn generate_all(
        &self,
        entities: &[EntityDefinition],
        enums: &[EnumDefinition],
    ) -> Result<DomainGenerationResult> {
        let mut result = DomainGenerationResult::new();

        // Generate for each entity
        for entity in entities {
            // gRPC client — opt-in only (REST is the default transport)
            if self.config.enable_grpc {
                let grpc_result = self.grpc_client_gen.generate(entity, enums)?;
                self.merge_result(&mut result, grpc_result);
            }

            // REST API client (default transport)
            let api_result = self.api_client_gen.generate(entity, enums)?;
            self.merge_result(&mut result, api_result);

            // Repository implementation
            let repo_result = self.repository_impl_gen.generate(entity, enums)?;
            self.merge_result(&mut result, repo_result);
        }

        // Shared REST transport + API utils (injectable, fetch-compatible)
        self.generate_http_support(&mut result)?;

        // Generate index files
        self.generate_index_files(entities, &mut result)?;

        // Generate gRPC module index (opt-in)
        if self.config.enable_grpc {
            let grpc_index = self.grpc_client_gen.generate_module_index(entities)?;
            self.merge_result(&mut result, grpc_index);
        }

        // Generate service initializer
        let service_init_result = self.service_init_gen.generate(entities, enums)?;
        self.merge_result(&mut result, service_init_result);

        Ok(result)
    }

    /// Merge a sub-result into the main result
    fn merge_result(&self, main: &mut DomainGenerationResult, sub: DomainGenerationResult) {
        main.files_generated.extend(sub.files_generated);
        main.dry_run_files.extend(sub.dry_run_files);
    }

    /// Emit the shared, injectable HTTP transport (`infrastructure/http/index.ts`)
    /// and the per-module API response helper (`infrastructure/{module}/api/utils.ts`).
    ///
    /// The transport is `fetch`-compatible, so an app can inject a ky instance
    /// (which extends fetch) to reuse its auth/refresh pipeline.
    fn generate_http_support(&self, result: &mut DomainGenerationResult) -> Result<()> {
        // Shared transport (module-agnostic, written once at shared/http)
        let http_dir = self.config.output_dir.join("shared").join("http");
        let http_index = r#"/**
 * Injectable HTTP transport (fetch-compatible).
 *
 * Generated API clients call `httpRequest`, which defaults to the global
 * `fetch`. An application can override it once at startup with any
 * fetch-compatible client (e.g. `ky`, which extends fetch) to reuse its
 * auth/refresh/error pipeline:
 *
 *   import ky from 'ky';
 *   import { setHttpRequest } from '@/generated/shared/http';
 *   setHttpRequest((input, init) => ky(input as string, init));
 *
 * @module shared/http
 */

export type HttpRequestFn = (input: string, init?: RequestInit) => Promise<Response>;

let _request: HttpRequestFn = (input, init) => fetch(input, init);

/** Override the transport used by every generated API client. */
export function setHttpRequest(fn: HttpRequestFn): void {
  _request = fn;
}

/** Perform an HTTP request through the configured transport. */
export function httpRequest(input: string, init?: RequestInit): Promise<Response> {
  return _request(input, init);
}
"#;
        self.write_simple(http_dir.join("index.ts"), http_index.to_string(), result);

        // Per-module API utils (PaginatedApiResponse envelope used by clients)
        let utils_dir = self.config.output_dir
            .join(&self.config.module).join("infrastructure")
            .join("api");
        let utils = r#"/**
 * Shared API response envelope types.
 *
 * @module infrastructure/api/utils
 */

/** Pagination metadata returned by the backend. */
export interface ApiResponseMeta {
  total: number;
  page: number;
  limit: number;
  total_pages: number;
}

/** Flat paginated API response: { success, data, meta }. */
export interface PaginatedApiResponse<T> {
  success: boolean;
  data: T[];
  meta: ApiResponseMeta;
  error?: string;
}
"#;
        self.write_simple(utils_dir.join("utils.ts"), utils.to_string(), result);

        Ok(())
    }

    /// Write a file, recording it in the result (respecting dry-run).
    fn write_simple(&self, path: std::path::PathBuf, content: String, result: &mut DomainGenerationResult) {
        if self.config.dry_run {
            result.dry_run_files.push(path);
        } else {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).ok();
            }
            fs::write(&path, content).ok();
            result.files_generated.push(path);
        }
    }

    /// Generate index files for infrastructure layer
    fn generate_index_files(
        &self,
        entities: &[EntityDefinition],
        result: &mut DomainGenerationResult,
    ) -> Result<()> {
        let base_dir = self.config.output_dir
            .join(&self.config.module).join("infrastructure");

        if !self.config.dry_run {
            fs::create_dir_all(&base_dir).ok();
        }

        // Generate main index
        let index_content = self.generate_index_content(entities);
        let index_path = base_dir.join("index.ts");

        result.add_file(index_path.clone(), self.config.dry_run);
        if !self.config.dry_run {
            fs::write(&index_path, index_content).ok();
        }

        Ok(())
    }

    fn generate_index_content(&self, entities: &[EntityDefinition]) -> String {
        let exports: Vec<String> = entities.iter()
            .map(|e| {
                let pascal = to_pascal_case(&e.name);
                format!(
                    "export * from './repository/{pascal}RepositoryImpl';",
                    pascal = pascal
                )
            })
            .collect();

        format!(
            "// Infrastructure layer exports for {} module\n// Generated by metaphor-webgen\n// Note: gRPC clients are exported from grpc/modules/{}\n\n{}\n",
            self.config.module,
            self.config.module,
            exports.join("\n")
        )
    }
}
