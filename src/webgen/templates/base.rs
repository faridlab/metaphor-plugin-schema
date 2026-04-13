//! Template definitions for generated code

/// React Query hook template
pub struct HookTemplate;

impl HookTemplate {
    /// Get the query hook template
    /// Uses Application Layer UseCases instead of direct gRPC calls
    pub fn query() -> &'static str {
        r#"/**
 * {{ENTITY_NAME}} Query Hook
 *
 * Generated React Query hook for fetching {{ENTITY_NAME}} data.
 * Uses Application Layer UseCases following Clean Architecture.
 *
 * @module application/hooks/{{MODULE_NAME}}/use{{ENTITY_NAME}}
 */

import { useQuery, UseQueryResult } from '@tanstack/react-query';
import { get{{ENTITY_NAME}}ByIdUseCase, list{{ENTITY_NAME}}UseCase } from '@webapp/application/{{MODULE_NAME}}/usecases/{{ENTITY_NAME}}UseCases';
import type { {{ENTITY_NAME}}, {{ENTITY_NAME}}QueryParams, {{ENTITY_NAME}}FilterParams } from '{{DOMAIN_IMPORT}}';
import type { PaginatedResponse } from '@webapp/shared/types/pagination';

/**
 * Query keys for {{ENTITY_NAME}} queries
 */
export const {{ENTITY_NAME_SNAKE}}QueryKeys = {
  all: ['{{MODULE_NAME}}', '{{ENTITY_NAME_SNAKE}}'] as const,
  lists: () => [...{{ENTITY_NAME_SNAKE}}QueryKeys.all, 'list'] as const,
  list: (filters?: string) => [...{{ENTITY_NAME_SNAKE}}QueryKeys.lists(), { filters }] as const,
  details: () => [...{{ENTITY_NAME_SNAKE}}QueryKeys.all, 'detail'] as const,
  detail: (id: string) => [...{{ENTITY_NAME_SNAKE}}QueryKeys.details(), id] as const,
  trash: () => [...{{ENTITY_NAME_SNAKE}}QueryKeys.all, 'trash'] as const,
} as const;

/**
 * Hook to fetch a single {{ENTITY_NAME}} by ID
 * Uses Application Layer UseCase
 */
export function use{{ENTITY_NAME}}(id: string): UseQueryResult<{{ENTITY_NAME}}> {
  return useQuery({
    queryKey: {{ENTITY_NAME_SNAKE}}QueryKeys.detail(id),
    queryFn: async () => {
      const result = await get{{ENTITY_NAME}}ByIdUseCase(id);
      if (!result.success || !result.data) {
        throw new Error(result.error?.message || 'Failed to fetch {{ENTITY_NAME}}');
      }
      return result.data;
    },
    enabled: !!id,
  });
}

/**
 * Hook to fetch a list of {{ENTITY_NAME}} entities
 * Uses Application Layer UseCase
 */
export function use{{ENTITY_NAME}}List(
  params?: {{ENTITY_NAME}}QueryParams,
  filters?: {{ENTITY_NAME}}FilterParams,
  options?: { enabled?: boolean }
): UseQueryResult<PaginatedResponse<{{ENTITY_NAME}}>> {
  return useQuery({
    queryKey: [...{{ENTITY_NAME_SNAKE}}QueryKeys.lists(), params, filters],
    queryFn: async () => {
      const result = await list{{ENTITY_NAME}}UseCase(params, filters);
      if (!result.success || !result.data) {
        throw new Error(result.error?.message || 'Failed to fetch {{ENTITY_NAME}} list');
      }
      return result.data;
    },
    enabled: options?.enabled ?? true,
  });
}

// Re-export mutations for convenience (import all hooks from one file)
export {
  useCreate{{ENTITY_NAME}},
  useUpdate{{ENTITY_NAME}},
  useDelete{{ENTITY_NAME}},
} from './use{{ENTITY_NAME}}Mutation';

// Alternative naming aliases for convenience
export { useDelete{{ENTITY_NAME}} as use{{ENTITY_NAME}}Delete } from './use{{ENTITY_NAME}}Mutation';
export { useCreate{{ENTITY_NAME}} as use{{ENTITY_NAME}}Create } from './use{{ENTITY_NAME}}Mutation';
export { useUpdate{{ENTITY_NAME}} as use{{ENTITY_NAME}}Update } from './use{{ENTITY_NAME}}Mutation';

// <<< CUSTOM: Add custom hooks here
// END CUSTOM
"#
    }

    /// Get the mutation hook template
    /// Uses Application Layer UseCases instead of direct gRPC calls
    pub fn mutation() -> &'static str {
        r#"/**
 * {{ENTITY_NAME}} Mutation Hook
 *
 * Generated React Query mutation hook for {{ENTITY_NAME}} operations.
 * Uses Application Layer UseCases following Clean Architecture.
 *
 * @module application/hooks/{{MODULE_NAME}}/use{{ENTITY_NAME}}Mutation
 */

import { useMutation, useQueryClient, UseMutationResult } from '@tanstack/react-query';
import type { UseMutationOptions } from '@tanstack/react-query';
import {
  create{{ENTITY_NAME}}UseCase,
  update{{ENTITY_NAME}}UseCase,
  patch{{ENTITY_NAME}}UseCase,
  delete{{ENTITY_NAME}}UseCase,
} from '@webapp/application/{{MODULE_NAME}}/usecases/{{ENTITY_NAME}}UseCases';
import { {{ENTITY_NAME_SNAKE}}QueryKeys } from './use{{ENTITY_NAME}}';
import type { {{ENTITY_NAME}} } from '{{DOMAIN_IMPORT}}';
import type { Create{{ENTITY_NAME}}Input, Update{{ENTITY_NAME}}Input, Patch{{ENTITY_NAME}}Input } from '{{DOMAIN_IMPORT}}';

/**
 * Hook to create a new {{ENTITY_NAME}}
 * Uses Application Layer UseCase
 */
export function useCreate{{ENTITY_NAME}}(
  options?: Partial<UseMutationOptions<unknown, Error, Create{{ENTITY_NAME}}Input>>
): UseMutationResult<
  unknown,
  Error,
  Create{{ENTITY_NAME}}Input
> {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (input: Create{{ENTITY_NAME}}Input) => create{{ENTITY_NAME}}UseCase(input),
    onSuccess: (result, variables, context) => {
      if (result && typeof result === 'object' && 'success' in result && result.success) {
        queryClient.invalidateQueries({ queryKey: {{ENTITY_NAME_SNAKE}}QueryKeys.lists() });
      }
      options?.onSuccess?.(result, variables, context);
    },
    onError: (error, variables, context) => {
      options?.onError?.(error, variables, context);
    },
  });
}

/**
 * Hook to update an existing {{ENTITY_NAME}}
 * Uses Application Layer UseCase
 */
export function useUpdate{{ENTITY_NAME}}(
  options?: Partial<UseMutationOptions<unknown, Error, { id: string; input: Update{{ENTITY_NAME}}Input }>>
): UseMutationResult<
  unknown,
  Error,
  { id: string; input: Update{{ENTITY_NAME}}Input }
> {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: ({ id, input }: { id: string; input: Update{{ENTITY_NAME}}Input }) =>
      update{{ENTITY_NAME}}UseCase(id, input),
    onSuccess: (result, variables, context) => {
      if (result && typeof result === 'object' && 'success' in result && result.success) {
        queryClient.invalidateQueries({ queryKey: {{ENTITY_NAME_SNAKE}}QueryKeys.detail(variables.id) });
        queryClient.invalidateQueries({ queryKey: {{ENTITY_NAME_SNAKE}}QueryKeys.lists() });
      }
      options?.onSuccess?.(result, variables, context);
    },
    onError: (error, variables, context) => {
      options?.onError?.(error, variables, context);
    },
  });
}

/**
 * Hook to patch (partial update) an existing {{ENTITY_NAME}}
 * Uses Application Layer UseCase
 */
export function usePatch{{ENTITY_NAME}}(
  options?: Partial<UseMutationOptions<unknown, Error, { id: string; input: Patch{{ENTITY_NAME}}Input }>>
): UseMutationResult<
  unknown,
  Error,
  { id: string; input: Patch{{ENTITY_NAME}}Input }
> {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: ({ id, input }: { id: string; input: Patch{{ENTITY_NAME}}Input }) =>
      patch{{ENTITY_NAME}}UseCase(id, input),
    onSuccess: (result, variables, context) => {
      if (result && typeof result === 'object' && 'success' in result && result.success) {
        queryClient.invalidateQueries({ queryKey: {{ENTITY_NAME_SNAKE}}QueryKeys.detail(variables.id) });
        queryClient.invalidateQueries({ queryKey: {{ENTITY_NAME_SNAKE}}QueryKeys.lists() });
      }
      options?.onSuccess?.(result, variables, context);
    },
    onError: (error, variables, context) => {
      options?.onError?.(error, variables, context);
    },
  });
}

/**
 * Hook to delete a {{ENTITY_NAME}}
 * Uses Application Layer UseCase
 */
export function useDelete{{ENTITY_NAME}}(
  options?: Partial<UseMutationOptions<unknown, Error, string>>
): UseMutationResult<
  unknown,
  Error,
  string
> {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (id: string) => delete{{ENTITY_NAME}}UseCase(id),
    onSuccess: (result, variables, context) => {
      if (result && typeof result === 'object' && 'success' in result && result.success) {
        queryClient.invalidateQueries({ queryKey: {{ENTITY_NAME_SNAKE}}QueryKeys.lists() });
      }
      options?.onSuccess?.(result, variables, context);
    },
    onError: (error, variables, context) => {
      options?.onError?.(error, variables, context);
    },
  });
}

// <<< CUSTOM: Add custom mutations here
// END CUSTOM
"#
    }

    /// Get additional soft-delete mutation hooks
    /// These are appended to the mutation hook file when the entity has soft_delete: true
    pub fn soft_delete_mutations() -> &'static str {
        r#"
// Soft-delete use case imports
import {
  softDelete{{ENTITY_NAME}}UseCase,
  restore{{ENTITY_NAME}}UseCase,
  permanentDelete{{ENTITY_NAME}}UseCase,
} from '@webapp/application/{{MODULE_NAME}}/usecases/{{ENTITY_NAME}}UseCases';

/**
 * Hook to soft delete a {{ENTITY_NAME}} (move to trash)
 * Uses Application Layer UseCase
 */
export function use{{ENTITY_NAME}}SoftDelete(
  options?: Partial<UseMutationOptions<unknown, Error, string>>
): UseMutationResult<
  unknown,
  Error,
  string
> {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (id: string) => softDelete{{ENTITY_NAME}}UseCase(id),
    onSuccess: (result, variables, context) => {
      if (result && typeof result === 'object' && 'success' in result && result.success) {
        queryClient.invalidateQueries({ queryKey: {{ENTITY_NAME_SNAKE}}QueryKeys.lists() });
        queryClient.invalidateQueries({ queryKey: {{ENTITY_NAME_SNAKE}}QueryKeys.trash() });
      }
      options?.onSuccess?.(result, variables, context);
    },
    onError: (error, variables, context) => {
      options?.onError?.(error, variables, context);
    },
  });
}

/**
 * Hook to restore a soft-deleted {{ENTITY_NAME}} from trash
 * Uses Application Layer UseCase
 */
export function use{{ENTITY_NAME}}Restore(
  options?: Partial<UseMutationOptions<unknown, Error, string>>
): UseMutationResult<
  unknown,
  Error,
  string
> {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (id: string) => restore{{ENTITY_NAME}}UseCase(id),
    onSuccess: (result, variables, context) => {
      if (result && typeof result === 'object' && 'success' in result && result.success) {
        queryClient.invalidateQueries({ queryKey: {{ENTITY_NAME_SNAKE}}QueryKeys.lists() });
        queryClient.invalidateQueries({ queryKey: {{ENTITY_NAME_SNAKE}}QueryKeys.trash() });
      }
      options?.onSuccess?.(result, variables, context);
    },
    onError: (error, variables, context) => {
      options?.onError?.(error, variables, context);
    },
  });
}

/**
 * Hook to permanently delete a {{ENTITY_NAME}} from trash
 * Uses Application Layer UseCase
 */
export function use{{ENTITY_NAME}}PermanentDelete(
  options?: Partial<UseMutationOptions<unknown, Error, string>>
): UseMutationResult<
  unknown,
  Error,
  string
> {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (id: string) => permanentDelete{{ENTITY_NAME}}UseCase(id),
    onSuccess: (result, variables, context) => {
      if (result && typeof result === 'object' && 'success' in result && result.success) {
        queryClient.invalidateQueries({ queryKey: {{ENTITY_NAME_SNAKE}}QueryKeys.lists() });
        queryClient.invalidateQueries({ queryKey: {{ENTITY_NAME_SNAKE}}QueryKeys.trash() });
      }
      options?.onSuccess?.(result, variables, context);
    },
    onError: (error, variables, context) => {
      options?.onError?.(error, variables, context);
    },
  });
}
"#
    }

    /// Get additional soft-delete query hooks
    /// These are appended to the query hook file when the entity has soft_delete: true
    pub fn soft_delete_queries() -> &'static str {
        r#"
import { listDeleted{{ENTITY_NAME}}UseCase } from '@webapp/application/{{MODULE_NAME}}/usecases/{{ENTITY_NAME}}UseCases';

/**
 * Hook to fetch soft-deleted {{ENTITY_NAME}} entities (trash)
 * Uses Application Layer UseCase - calls /trash endpoint via getDeleted()
 */
export function use{{ENTITY_NAME}}TrashList(
  params?: {{ENTITY_NAME}}QueryParams,
  filters?: {{ENTITY_NAME}}FilterParams,
  options?: { enabled?: boolean }
): UseQueryResult<PaginatedResponse<{{ENTITY_NAME}}>> {
  return useQuery({
    queryKey: [...{{ENTITY_NAME_SNAKE}}QueryKeys.trash(), params, filters],
    queryFn: async () => {
      const result = await listDeleted{{ENTITY_NAME}}UseCase(params, filters);
      if (!result.success || !result.data) {
        throw new Error(result.error?.message || 'Failed to fetch {{ENTITY_NAME}} trash list');
      }
      return result.data;
    },
    enabled: options?.enabled ?? true,
  });
}
"#
    }
}

/// Zod validation schema template
pub struct SchemaTemplate;

impl SchemaTemplate {
    /// Get the schema template
    pub fn schema() -> &'static str {
        r#"/**
 * {{ENTITY_NAME}} Validation Schema
 *
 * Generated Zod schema for {{ENTITY_NAME}} validation.
 */

import { z } from 'zod';

/**
 * {{ENTITY_NAME}} schema for validation
 */
export const {{ENTITY_NAME_SNAKE}}Schema = z.object({
  id: z.string().uuid().optional(),
  // <<< CUSTOM: Add fields based on your proto definition
  // Example:
  // name: z.string().min(1).max(100),
  // email: z.string().email(),
  // createdAt: z.date().optional(),
  // END CUSTOM
});

/**
 * Schema for creating a {{ENTITY_NAME}}
 */
export const create{{ENTITY_NAME}}Schema = {{ENTITY_NAME_SNAKE}}Schema.omit({ id: true });

/**
 * Schema for updating a {{ENTITY_NAME}}
 */
export const update{{ENTITY_NAME}}Schema = {{ENTITY_NAME_SNAKE}}Schema.partial().required({ id: true });

/**
 * Type inference from schema
 */
export type {{ENTITY_NAME}}Input = z.infer<typeof {{ENTITY_NAME_SNAKE}}Schema>;
export type Create{{ENTITY_NAME}}Input = z.infer<typeof create{{ENTITY_NAME}}Schema>;
export type Update{{ENTITY_NAME}}Input = z.infer<typeof update{{ENTITY_NAME}}Schema>;

// <<< CUSTOM: Add custom schemas here
// END CUSTOM
"#
    }
}

/// Form component template
pub struct FormTemplate;

impl FormTemplate {
    /// Get the create form template
    pub fn create() -> &'static str {
        r#"/**
 * {{ENTITY_NAME}} Create Form Component
 *
 * Generated form component for creating a {{ENTITY_NAME}}.
 */

import { useForm } from 'react-hook-form';
import { zodResolver } from '@hookform/resolvers/zod';
import { Box, Button, Stack } from '@/components/ui';
import { create{{ENTITY_NAME}}Schema, type Create{{ENTITY_NAME}}Input } from '@webapp/application/validators/{{MODULE_NAME}}/{{ENTITY_NAME_SNAKE}}.schema';
import { useCreate{{ENTITY_NAME}} } from '@webapp/application/hooks/{{MODULE_NAME}}/use{{ENTITY_NAME}}Mutation';

export interface {{ENTITY_NAME}}CreateFormProps {
  onSuccess?: () => void;
  onCancel?: () => void;
}

export function {{ENTITY_NAME}}CreateForm({ onSuccess, onCancel }: {{ENTITY_NAME}}CreateFormProps) {
  const { mutate: create{{ENTITY_NAME}}, isPending } = useCreate{{ENTITY_NAME}}();

  const {
    register,
    handleSubmit,
    formState: { errors },
  } = useForm<Create{{ENTITY_NAME}}Input>({
    resolver: zodResolver(create{{ENTITY_NAME}}Schema),
  });

  const onSubmit = (data: Create{{ENTITY_NAME}}Input) => {
    create{{ENTITY_NAME}}(data);
    if (onSuccess) onSuccess();
  };

  return (
    <Box component="form" onSubmit={handleSubmit(onSubmit)}>
      <Stack spacing={3}>
        {/* <<< CUSTOM: Add form fields based on your entity */}
        {/* Example:
        <TextField
          label="Name"
          {...register('name')}
          error={!!errors.name}
          helperText={errors.name?.message}
          fullWidth
        />
        */}

        <Stack direction="row" spacing={2} justifyContent="flex-end">
          {onCancel && (
            <Button onClick={onCancel} disabled={isPending}>
              Cancel
            </Button>
          )}
          <Button
            type="submit"
            variant="contained"
            disabled={isPending}
          >
            {isPending ? 'Creating...' : 'Create {{ENTITY_NAME}}'}
          </Button>
        </Stack>

        {/* END CUSTOM */}
      </Stack>
    </Box>
  );
}
"#
    }

    /// Get the edit form template
    pub fn edit() -> &'static str {
        r#"/**
 * {{ENTITY_NAME}} Edit Form Component
 *
 * Generated form component for editing a {{ENTITY_NAME}}.
 */

import { useForm } from 'react-hook-form';
import { zodResolver } from '@hookform/resolvers/zod';
import { Box, Button, Stack } from '@/components/ui';
import { update{{ENTITY_NAME}}Schema, type Update{{ENTITY_NAME}}Input } from '@webapp/application/validators/{{MODULE_NAME}}/{{ENTITY_NAME_SNAKE}}.schema';
import { useUpdate{{ENTITY_NAME}} } from '@webapp/application/hooks/{{MODULE_NAME}}/use{{ENTITY_NAME}}Mutation';
import type { {{ENTITY_NAME}} } from '{{DOMAIN_IMPORT}}';
import { useEffect } from 'react';

export interface {{ENTITY_NAME}}EditFormProps {
  {{ENTITY_NAME_SNAKE}}: {{ENTITY_NAME}};
  onSuccess?: () => void;
  onCancel?: () => void;
}

export function {{ENTITY_NAME}}EditForm({ {{ENTITY_NAME_SNAKE}}, onSuccess, onCancel }: {{ENTITY_NAME}}EditFormProps) {
  const { mutate: update{{ENTITY_NAME}}, isPending } = useUpdate{{ENTITY_NAME}}();

  const {
    register,
    handleSubmit,
    reset,
    formState: { errors },
  } = useForm<Update{{ENTITY_NAME}}Input>({
    resolver: zodResolver(update{{ENTITY_NAME}}Schema),
    defaultValues: {
      id: {{ENTITY_NAME_SNAKE}}.id,
    },
  });

  useEffect(() => {
    reset({ id: {{ENTITY_NAME_SNAKE}}.id });
  }, [{{ENTITY_NAME_SNAKE}}, reset]);

  const onSubmit = (data: Update{{ENTITY_NAME}}Input) => {
    update{{ENTITY_NAME}}({ id: {{ENTITY_NAME_SNAKE}}.id, input: data });
    if (onSuccess) onSuccess();
  };

  return (
    <Box component="form" onSubmit={handleSubmit(onSubmit)}>
      <Stack spacing={3}>
        {/* <<< CUSTOM: Add form fields based on your entity */}
        {/* Example:
        <TextField
          label="Name"
          {...register('name')}
          error={!!errors.name}
          helperText={errors.name?.message}
          defaultValue={{{ENTITY_NAME_SNAKE}}.name}
          fullWidth
        />
        */}

        <Stack direction="row" spacing={2} justifyContent="flex-end">
          {onCancel && (
            <Button onClick={onCancel} disabled={isPending}>
              Cancel
            </Button>
          )}
          <Button
            type="submit"
            variant="contained"
            disabled={isPending}
          >
            {isPending ? 'Saving...' : 'Save Changes'}
          </Button>
        </Stack>

        {/* END CUSTOM */}
      </Stack>
    </Box>
  );
}
"#
    }
}

/// Page component template
pub struct PageTemplate;

impl PageTemplate {
    /// Get the list page template
    pub fn list() -> &'static str {
        r#"/**
 * {{ENTITY_NAME}} List Page
 *
 * Generated list page for {{ENTITY_NAME}} entities.
 */

import { useState } from 'react';
import {
  Box,
  Button,
  Container,
  Stack,
  Typography,
} from '@/components/ui';
import { Plus } from '@/components/ui';
import { use{{ENTITY_NAME}}List } from '@webapp/application/hooks/{{MODULE_NAME}}/use{{ENTITY_NAME}}';
import { useNavigate } from 'react-router-dom';

export function {{ENTITY_NAME}}ListPage() {
  const navigate = useNavigate();
  const [page, setPage] = useState(1);
  const [pageSize, setPageSize] = useState(20);

  const { data, isLoading, error } = use{{ENTITY_NAME}}List({
    page,
    pageSize,
  });

  const handleCreate = () => {
    navigate(`/{{MODULE_NAME}}/{{ENTITY_NAME_SNAKE}}/create`);
  };

  if (isLoading) {
    return <div>Loading...</div>;
  }

  if (error) {
    return <div>Error: {error.message}</div>;
  }

  return (
    <Container maxWidth="xl">
      <Stack spacing={3}>
        <Stack direction="row" justifyContent="space-between" alignItems="center">
          <Typography variant="h4">{{ENTITY_NAME}} List</Typography>
          <Button
            variant="contained"
            startIcon={<Plus />}
            onClick={handleCreate}
          >
            Add {{ENTITY_NAME}}
          </Button>
        </Stack>

        {/* <<< CUSTOM: Add data table here */}
        <Box>
          <Typography>Found {data?.total || 0} {{ENTITY_NAME_SNAKE}} entities</Typography>
          {/* Example: <DataTable columns={columns} rows={data?.data || []} /> */}
        </Box>
        {/* END CUSTOM */}

      </Stack>
    </Container>
  );
}
"#
    }

    /// Get the detail page template
    pub fn detail() -> &'static str {
        r#"/**
 * {{ENTITY_NAME}} Detail Page
 *
 * Generated detail page for viewing a single {{ENTITY_NAME}}.
 */

import { Box, Container, Stack, Typography } from '@/components/ui';
import { useParams, useNavigate } from 'react-router-dom';
import { use{{ENTITY_NAME}} } from '@webapp/application/hooks/{{MODULE_NAME}}/use{{ENTITY_NAME}}';
import { ArrowBack, IconButton } from '@/components/ui';

export function {{ENTITY_NAME}}DetailPage() {
  const { id } = useParams<{ id: string }>();
  const navigate = useNavigate();

  const { data: {{ENTITY_NAME_SNAKE}}, isLoading, error } = use{{ENTITY_NAME}}(id || '');

  if (isLoading) {
    return <div>Loading...</div>;
  }

  if (error) {
    return <div>Error: {error.message}</div>;
  }

  if (!{{ENTITY_NAME_SNAKE}}) {
    return <div>{{ENTITY_NAME}} not found</div>;
  }

  return (
    <Container maxWidth="xl">
      <Stack spacing={3}>
        <Stack direction="row" alignItems="center" spacing={2}>
          <IconButton onClick={() => navigate(`/{{MODULE_NAME}}/{{ENTITY_NAME_SNAKE}}`)}>
            <ArrowBack />
          </IconButton>
          <Typography variant="h4">{{ENTITY_NAME}} Details</Typography>
        </Stack>

        {/* <<< CUSTOM: Add detail fields here */}
        <Box>
          <Typography>ID: {{ENTITY_NAME_SNAKE}}.id</Typography>
          {/* Add more fields based on your entity */}
        </Box>
        {/* END CUSTOM */}

      </Stack>
    </Container>
  );
}
"#
    }

    /// Get the create page template
    pub fn create() -> &'static str {
        r#"/**
 * {{ENTITY_NAME}} Create Page
 *
 * Generated create page for creating a new {{ENTITY_NAME}}.
 */

import { Container, Stack, Typography } from '@/components/ui';
import { {{ENTITY_NAME}}CreateForm } from '@webapp/presentation/components/forms/{{MODULE_NAME}}/{{ENTITY_NAME}}CreateForm';
import { useNavigate } from 'react-router-dom';

export function {{ENTITY_NAME}}CreatePage() {
  const navigate = useNavigate();

  const handleSuccess = () => {
    navigate(`/{{MODULE_NAME}}/{{ENTITY_NAME_SNAKE}}`);
  };

  const handleCancel = () => {
    navigate(`/{{MODULE_NAME}}/{{ENTITY_NAME_SNAKE}}`);
  };

  return (
    <Container maxWidth="md">
      <Stack spacing={3}>
        <Typography variant="h4">Create {{ENTITY_NAME}}</Typography>

        <{{ENTITY_NAME}}CreateForm
          onSuccess={handleSuccess}
          onCancel={handleCancel}
        />

      </Stack>
    </Container>
  );
}
"#
    }

    /// Get the edit page template
    pub fn edit() -> &'static str {
        r#"/**
 * {{ENTITY_NAME}} Edit Page
 *
 * Generated edit page for editing an existing {{ENTITY_NAME}}.
 */

import { Container, Stack, Typography } from '@/components/ui';
import { {{ENTITY_NAME}}EditForm } from '@webapp/presentation/components/forms/{{MODULE_NAME}}/{{ENTITY_NAME}}EditForm';
import { useNavigate, useParams } from 'react-router-dom';
import { use{{ENTITY_NAME}} } from '@webapp/application/hooks/{{MODULE_NAME}}/use{{ENTITY_NAME}}';

export function {{ENTITY_NAME}}EditPage() {
  const { id } = useParams<{ id: string }>();
  const navigate = useNavigate();

  const { data: {{ENTITY_NAME_SNAKE}}, isLoading } = use{{ENTITY_NAME}}(id || '');

  const handleSuccess = () => {
    navigate(`/{{MODULE_NAME}}/{{ENTITY_NAME_SNAKE}}`);
  };

  const handleCancel = () => {
    navigate(`/{{MODULE_NAME}}/{{ENTITY_NAME_SNAKE}}`);
  };

  if (isLoading) {
    return <div>Loading...</div>;
  }

  if (!{{ENTITY_NAME_SNAKE}}) {
    return <div>{{ENTITY_NAME}} not found</div>;
  }

  return (
    <Container maxWidth="md">
      <Stack spacing={3}>
        <Typography variant="h4">Edit {{ENTITY_NAME}}</Typography>

        <{{ENTITY_NAME}}EditForm
          {{ENTITY_NAME_SNAKE}}={{{ENTITY_NAME_SNAKE}}}
          onSuccess={handleSuccess}
          onCancel={handleCancel}
        />

      </Stack>
    </Container>
  );
}
"#
    }
}

/// Template placeholder replacer
pub struct TemplateReplacer {
    entity_pascal: String,
    entity_snake: String,
    module: String,
    domain_import: String,
}

impl TemplateReplacer {
    /// Create a new replacer
    pub fn new(entity_pascal: String, entity_snake: String, module: String, domain_import: String) -> Self {
        Self {
            entity_pascal,
            entity_snake,
            module,
            domain_import,
        }
    }

    /// Replace placeholders in a template
    pub fn replace(&self, template: &str) -> String {
        let mut result = template.to_string();
        result = result.replace("{{ENTITY_NAME}}", &self.entity_pascal);
        result = result.replace("{{ENTITY_NAME_SNAKE}}", &self.entity_snake);
        result = result.replace("{{ENTITY_NAME_CAMEL}}", &crate::webgen::parser::to_camel_case(&self.entity_pascal));
        result = result.replace("{{MODULE_NAME}}", &self.module);
        result = result.replace("{{DOMAIN_IMPORT}}", &self.domain_import);
        result
    }
}
