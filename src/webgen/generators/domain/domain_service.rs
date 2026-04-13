//! Domain Service generator for TypeScript domain layer
//!
//! Generates service interfaces with React Query hooks for data fetching.

use std::fs;

use crate::webgen::ast::entity::EntityDefinition;
use crate::webgen::config::Config;
use crate::webgen::error::Result;
use crate::webgen::parser::{to_pascal_case, to_camel_case, to_snake_case};
use super::DomainGenerationResult;

/// Generator for domain service interfaces with React Query hooks
pub struct DomainServiceGenerator {
    config: Config,
}

impl DomainServiceGenerator {
    /// Create a new domain service generator
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    /// Generate domain service for an entity
    pub fn generate(&self, entity: &EntityDefinition) -> Result<DomainGenerationResult> {
        let mut result = DomainGenerationResult::new();

        let entity_pascal = to_pascal_case(&entity.name);
        let service_dir = self.config.output_dir
            .join("domain")
            .join(&self.config.module)
            .join("service");

        if !self.config.dry_run {
            fs::create_dir_all(&service_dir).ok();
        }

        let content = self.generate_service_content(entity);
        let path = service_dir.join(format!("{}Service.ts", entity_pascal));

        result.add_file(path.clone(), self.config.dry_run);

        if !self.config.dry_run {
            fs::write(&path, content).ok();
        }

        Ok(result)
    }

    /// Generate domain service content with React Query hooks
    fn generate_service_content(&self, entity: &EntityDefinition) -> String {
        let entity_pascal = to_pascal_case(&entity.name);
        let entity_camel = to_camel_case(&entity.name);
        let entity_snake = to_snake_case(&entity.name);
        let has_soft_delete = entity.has_soft_delete();

        // Build soft-delete query key line
        let soft_delete_query_key = if has_soft_delete {
            format!(
                "\n  trash: () => [...{}Keys.all, 'trash'] as const,",
                entity_camel
            )
        } else {
            String::new()
        };

        // Build soft-delete service interface methods
        let soft_delete_service_methods = if has_soft_delete {
            format!(
                r#"
  softDelete(id: string): Promise<{ep}>;
  restore(id: string): Promise<{ep}>;
  permanentDelete(id: string): Promise<void>;
  getDeleted(params?: {ep}QueryParams, filters?: {ep}FilterParams): Promise<Paginated{ep}Response>;"#,
                ep = entity_pascal
            )
        } else {
            String::new()
        };

        // Build the main content
        let mut content = format!(
r#"/**
 * {entity_pascal} Domain Service
 *
 * React Query hooks and service interface for {entity_pascal} operations.
 * Provides type-safe data fetching, caching, and mutation capabilities.
 *
 * @module {module}/service/{entity_pascal}Service
 */

import {{
  useQuery,
  useMutation,
  useQueryClient,
  useInfiniteQuery,
  type UseQueryOptions,
  type UseMutationOptions,
  type UseInfiniteQueryOptions,
}} from '@tanstack/react-query';
import type {{
  {entity_pascal},
  Create{entity_pascal}Input,
  Update{entity_pascal}Input,
  Patch{entity_pascal}Input,
  {entity_pascal}QueryParams,
  {entity_pascal}FilterParams,
}} from '../entity/{entity_pascal}.schema';
import type {{ Paginated{entity_pascal}Response }} from '../repository/{entity_pascal}Repository';

// ============================================================================
// Query Keys
// ============================================================================

/**
 * Query key factory for {entity_pascal}
 *
 * Follows React Query best practices for hierarchical cache invalidation.
 */
export const {entity_camel}Keys = {{
  all: ['{entity_snake}'] as const,
  lists: () => [...{entity_camel}Keys.all, 'list'] as const,
  list: (params?: {entity_pascal}QueryParams, filters?: {entity_pascal}FilterParams) =>
    [...{entity_camel}Keys.lists(), {{ params, filters }}] as const,
  details: () => [...{entity_camel}Keys.all, 'detail'] as const,
  detail: (id: string) => [...{entity_camel}Keys.details(), id] as const,
  infinite: (params?: {entity_pascal}QueryParams, filters?: {entity_pascal}FilterParams) =>
    [...{entity_camel}Keys.all, 'infinite', {{ params, filters }}] as const,{soft_delete_query_key}
}};

// ============================================================================
// Service Interface
// ============================================================================

/**
 * {entity_pascal} Service Interface
 *
 * Define your API client implementation for this interface.
 */
export interface {entity_pascal}Service {{
  getById(id: string): Promise<{entity_pascal}>;
  getAll(params?: {entity_pascal}QueryParams, filters?: {entity_pascal}FilterParams): Promise<Paginated{entity_pascal}Response>;
  create(input: Create{entity_pascal}Input): Promise<{entity_pascal}>;
  update(id: string, input: Update{entity_pascal}Input): Promise<{entity_pascal}>;
  patch(id: string, input: Patch{entity_pascal}Input): Promise<{entity_pascal}>;
  delete(id: string): Promise<void>;
  exists(id: string): Promise<boolean>;
  count(filters?: {entity_pascal}FilterParams): Promise<number>;{soft_delete_service_methods}
}}

// ============================================================================
// Service Instance (inject your implementation)
// ============================================================================

let _service: {entity_pascal}Service | null = null;

/**
 * Set the {entity_pascal} service implementation
 */
export function set{entity_pascal}Service(service: {entity_pascal}Service): void {{
  _service = service;
}}

/**
 * Get the {entity_pascal} service instance
 * @throws Error if service not initialized
 */
export function get{entity_pascal}Service(): {entity_pascal}Service {{
  if (!_service) {{
    throw new Error(
      '{entity_pascal}Service not initialized. Call set{entity_pascal}Service() first.'
    );
  }}
  return _service;
}}

// ============================================================================
// Query Hooks
// ============================================================================

/**
 * Hook to fetch a single {entity_pascal} by ID
 */
export function use{entity_pascal}(
  id: string,
  options?: Omit<UseQueryOptions<{entity_pascal}, Error>, 'queryKey' | 'queryFn'>
) {{
  const service = get{entity_pascal}Service();

  return useQuery({{
    queryKey: {entity_camel}Keys.detail(id),
    queryFn: () => service.getById(id),
    enabled: !!id,
    ...options,
  }});
}}

/**
 * Hook to fetch paginated list of {entity_pascal} entities
 */
export function use{entity_pascal}List(
  params?: {entity_pascal}QueryParams,
  filters?: {entity_pascal}FilterParams,
  options?: Omit<UseQueryOptions<Paginated{entity_pascal}Response, Error>, 'queryKey' | 'queryFn'>
) {{
  const service = get{entity_pascal}Service();

  return useQuery({{
    queryKey: {entity_camel}Keys.list(params, filters),
    queryFn: () => service.getAll(params, filters),
    ...options,
  }});
}}

/**
 * Hook for infinite scrolling list of {entity_pascal} entities
 */
export function use{entity_pascal}InfiniteList(
  params?: Omit<{entity_pascal}QueryParams, 'page'>,
  filters?: {entity_pascal}FilterParams,
  options?: Omit<
    UseInfiniteQueryOptions<Paginated{entity_pascal}Response, Error>,
    'queryKey' | 'queryFn' | 'getNextPageParam' | 'initialPageParam'
  >
) {{
  const service = get{entity_pascal}Service();

  return useInfiniteQuery({{
    queryKey: {entity_camel}Keys.infinite(params, filters),
    queryFn: ({{ pageParam = 1 }}) =>
      service.getAll({{ ...params, page: pageParam as number }}, filters),
    initialPageParam: 1,
    getNextPageParam: (lastPage) =>
      lastPage.hasNext ? lastPage.page + 1 : undefined,
    getPreviousPageParam: (firstPage) =>
      firstPage.hasPrev ? firstPage.page - 1 : undefined,
    ...options,
  }});
}}

/**
 * Hook to check if a {entity_pascal} exists
 */
export function use{entity_pascal}Exists(
  id: string,
  options?: Omit<UseQueryOptions<boolean, Error>, 'queryKey' | 'queryFn'>
) {{
  const service = get{entity_pascal}Service();

  return useQuery({{
    queryKey: [...{entity_camel}Keys.detail(id), 'exists'],
    queryFn: () => service.exists(id),
    enabled: !!id,
    ...options,
  }});
}}

/**
 * Hook to get {entity_pascal} count
 */
export function use{entity_pascal}Count(
  filters?: {entity_pascal}FilterParams,
  options?: Omit<UseQueryOptions<number, Error>, 'queryKey' | 'queryFn'>
) {{
  const service = get{entity_pascal}Service();

  return useQuery({{
    queryKey: [...{entity_camel}Keys.lists(), 'count', filters],
    queryFn: () => service.count(filters),
    ...options,
  }});
}}

// ============================================================================
// Mutation Hooks
// ============================================================================

/**
 * Hook to create a new {entity_pascal}
 */
export function useCreate{entity_pascal}(
  options?: Omit<
    UseMutationOptions<{entity_pascal}, Error, Create{entity_pascal}Input>,
    'mutationFn'
  >
) {{
  const service = get{entity_pascal}Service();
  const queryClient = useQueryClient();

  return useMutation({{
    mutationFn: (input: Create{entity_pascal}Input) => service.create(input),
    onSuccess: (data, _variables, _context) => {{
      // Invalidate list queries to refresh data
      queryClient.invalidateQueries({{ queryKey: {entity_camel}Keys.lists() }});
      // Optionally pre-populate the cache for the new entity
      queryClient.setQueryData({entity_camel}Keys.detail(data.id), data);
    }},
    ...options,
  }});
}}

/**
 * Hook to update an existing {entity_pascal}
 */
export function useUpdate{entity_pascal}(
  options?: Omit<
    UseMutationOptions<{entity_pascal}, Error, {{ id: string; input: Update{entity_pascal}Input }}>,
    'mutationFn'
  >
) {{
  const service = get{entity_pascal}Service();
  const queryClient = useQueryClient();

  return useMutation({{
    mutationFn: ({{ id, input }}) => service.update(id, input),
    onSuccess: (data, variables, _context) => {{
      // Update the specific entity in cache
      queryClient.setQueryData({entity_camel}Keys.detail(variables.id), data);
      // Invalidate list queries
      queryClient.invalidateQueries({{ queryKey: {entity_camel}Keys.lists() }});
    }},
    ...options,
  }});
}}

/**
 * Hook to patch (partial update) an existing {entity_pascal}
 */
export function usePatch{entity_pascal}(
  options?: Omit<
    UseMutationOptions<{entity_pascal}, Error, {{ id: string; input: Patch{entity_pascal}Input }}>,
    'mutationFn'
  >
) {{
  const service = get{entity_pascal}Service();
  const queryClient = useQueryClient();

  return useMutation({{
    mutationFn: ({{ id, input }}) => service.patch(id, input),
    onSuccess: (data, variables, _context) => {{
      // Update the specific entity in cache
      queryClient.setQueryData({entity_camel}Keys.detail(variables.id), data);
      // Invalidate list queries
      queryClient.invalidateQueries({{ queryKey: {entity_camel}Keys.lists() }});
    }},
    ...options,
  }});
}}

/**
 * Hook to delete a {entity_pascal}
 */
export function useDelete{entity_pascal}(
  options?: Omit<UseMutationOptions<void, Error, string>, 'mutationFn'>
) {{
  const service = get{entity_pascal}Service();
  const queryClient = useQueryClient();

  return useMutation({{
    mutationFn: (id: string) => service.delete(id),
    onSuccess: (_data, id, _context) => {{
      // Remove from cache
      queryClient.removeQueries({{ queryKey: {entity_camel}Keys.detail(id) }});
      // Invalidate list queries
      queryClient.invalidateQueries({{ queryKey: {entity_camel}Keys.lists() }});
    }},
    ...options,
  }});
}}

// ============================================================================
// Prefetch Utilities
// ============================================================================

/**
 * Prefetch a single {entity_pascal} for faster navigation
 */
export async function prefetch{entity_pascal}(
  queryClient: ReturnType<typeof useQueryClient>,
  id: string
): Promise<void> {{
  const service = get{entity_pascal}Service();
  await queryClient.prefetchQuery({{
    queryKey: {entity_camel}Keys.detail(id),
    queryFn: () => service.getById(id),
  }});
}}

/**
 * Prefetch {entity_pascal} list
 */
export async function prefetch{entity_pascal}List(
  queryClient: ReturnType<typeof useQueryClient>,
  params?: {entity_pascal}QueryParams,
  filters?: {entity_pascal}FilterParams
): Promise<void> {{
  const service = get{entity_pascal}Service();
  await queryClient.prefetchQuery({{
    queryKey: {entity_camel}Keys.list(params, filters),
    queryFn: () => service.getAll(params, filters),
  }});
}}

// ============================================================================
// Cache Utilities
// ============================================================================

/**
 * Invalidate all {entity_pascal} queries
 */
export function invalidate{entity_pascal}Queries(
  queryClient: ReturnType<typeof useQueryClient>
): Promise<void> {{
  return queryClient.invalidateQueries({{ queryKey: {entity_camel}Keys.all }});
}}

/**
 * Get cached {entity_pascal} by ID
 */
export function getCached{entity_pascal}(
  queryClient: ReturnType<typeof useQueryClient>,
  id: string
): {entity_pascal} | undefined {{
  return queryClient.getQueryData({entity_camel}Keys.detail(id));
}}

/**
 * Set {entity_pascal} in cache
 */
export function setCached{entity_pascal}(
  queryClient: ReturnType<typeof useQueryClient>,
  data: {entity_pascal}
): void {{
  queryClient.setQueryData({entity_camel}Keys.detail(data.id), data);
}}

// <<< CUSTOM: Add custom service methods here
// END CUSTOM
"#,
            entity_pascal = entity_pascal,
            entity_camel = entity_camel,
            entity_snake = entity_snake,
            module = self.config.module,
            soft_delete_query_key = soft_delete_query_key,
            soft_delete_service_methods = soft_delete_service_methods,
        );

        // Append soft-delete hooks if entity has soft_delete
        if has_soft_delete {
            content.push_str(&Self::generate_soft_delete_hooks(&entity_pascal, &entity_camel));
        }

        content
    }

    /// Generate soft-delete hooks content (appended after main content)
    fn generate_soft_delete_hooks(entity_pascal: &str, entity_camel: &str) -> String {
        format!(
r#"
// ============================================================================
// Soft Delete Hooks
// ============================================================================

/**
 * Hook to soft delete a {ep} (move to trash)
 */
export function useSoftDelete{ep}(
  options?: Omit<UseMutationOptions<{ep}, Error, string>, 'mutationFn'>
) {{
  const service = get{ep}Service();
  const queryClient = useQueryClient();

  return useMutation({{
    mutationFn: (id: string) => service.softDelete(id),
    onSuccess: (_data, id, _context) => {{
      queryClient.removeQueries({{ queryKey: {ec}Keys.detail(id) }});
      queryClient.invalidateQueries({{ queryKey: {ec}Keys.lists() }});
      queryClient.invalidateQueries({{ queryKey: {ec}Keys.trash() }});
    }},
    ...options,
  }});
}}

/**
 * Hook to restore a soft-deleted {ep} from trash
 */
export function useRestore{ep}(
  options?: Omit<UseMutationOptions<{ep}, Error, string>, 'mutationFn'>
) {{
  const service = get{ep}Service();
  const queryClient = useQueryClient();

  return useMutation({{
    mutationFn: (id: string) => service.restore(id),
    onSuccess: (_data, _id, _context) => {{
      queryClient.invalidateQueries({{ queryKey: {ec}Keys.lists() }});
      queryClient.invalidateQueries({{ queryKey: {ec}Keys.trash() }});
    }},
    ...options,
  }});
}}

/**
 * Hook to permanently delete a {ep} from trash
 */
export function usePermanentDelete{ep}(
  options?: Omit<UseMutationOptions<void, Error, string>, 'mutationFn'>
) {{
  const service = get{ep}Service();
  const queryClient = useQueryClient();

  return useMutation({{
    mutationFn: (id: string) => service.permanentDelete(id),
    onSuccess: (_data, id, _context) => {{
      queryClient.removeQueries({{ queryKey: {ec}Keys.detail(id) }});
      queryClient.invalidateQueries({{ queryKey: {ec}Keys.trash() }});
    }},
    ...options,
  }});
}}

/**
 * Hook to fetch paginated list of soft-deleted {ep} entities (trash)
 */
export function use{ep}DeletedList(
  params?: {ep}QueryParams,
  filters?: {ep}FilterParams,
  options?: Omit<UseQueryOptions<Paginated{ep}Response, Error>, 'queryKey' | 'queryFn'>
) {{
  const service = get{ep}Service();

  return useQuery({{
    queryKey: [...{ec}Keys.trash(), {{ params, filters }}],
    queryFn: () => service.getDeleted(params, filters),
    ...options,
  }});
}}
"#,
            ep = entity_pascal,
            ec = entity_camel,
        )
    }
}
