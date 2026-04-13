//! CRUD pages generator
//!
//! Generates complete CRUD page components (List, Create, Edit, Detail).
//! Uses Joy UI components from @/components/ui

use std::fs;

use crate::webgen::ast::entity::{EntityDefinition, EnumDefinition};
use crate::webgen::config::Config;
use crate::webgen::error::Result;
use crate::webgen::parser::{to_pascal_case, to_camel_case, to_snake_case};
use crate::webgen::generators::domain::DomainGenerationResult;

/// Generator for CRUD page components
pub struct CrudPagesGenerator {
    config: Config,
}

impl CrudPagesGenerator {
    /// Create a new CRUD pages generator
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    /// Generate CRUD pages for an entity
    pub fn generate(
        &self,
        entity: &EntityDefinition,
        _enums: &[EnumDefinition],
    ) -> Result<DomainGenerationResult> {
        let mut result = DomainGenerationResult::new();

        let entity_pascal = to_pascal_case(&entity.name);
        let pages_dir = self.config.output_dir
            .join("presentation")
            .join("pages")
            .join(&self.config.module);

        if !self.config.dry_run {
            fs::create_dir_all(&pages_dir).ok();
        }

        // Generate pages file
        let content = self.generate_pages_content(entity);
        let file_path = pages_dir.join(format!("{}Pages.tsx", entity_pascal));

        result.add_file(file_path.clone(), self.config.dry_run);

        if !self.config.dry_run {
            fs::write(&file_path, content).ok();
        }

        Ok(result)
    }

    /// Generate pages content using Joy UI components
    fn generate_pages_content(&self, entity: &EntityDefinition) -> String {
        let entity_pascal = to_pascal_case(&entity.name);
        let entity_camel = to_camel_case(&entity.name);
        let entity_snake = to_snake_case(&entity.name);
        let has_soft_delete = entity.has_soft_delete();

        // Additional imports for soft delete
        let soft_delete_imports = if has_soft_delete {
            format!(
r#"  use{entity_pascal}TrashList,
  use{entity_pascal}Restore,
  use{entity_pascal}PermanentDelete,"#,
                entity_pascal = entity_pascal,
            )
        } else {
            String::new()
        };

        // Additional icon imports for soft delete
        let soft_delete_icons = if has_soft_delete {
            ", DeleteSweep, RestoreFromTrash"
        } else {
            ""
        };

        // Build reusable hook imports based on soft delete support
        let reusable_hooks_imports = if has_soft_delete {
            "import { useListOperations, useDetailOperations, useTrashOperations } from '@webapp/application/hooks/common';"
        } else {
            "import { useListOperations, useDetailOperations } from '@webapp/application/hooks/common';"
        };

        // Build reusable component imports
        let reusable_components_imports = if has_soft_delete {
            r#"import { ListPageActions, DeleteDialogs } from '@webapp/presentation/components/list';
import { DangerZone, DeleteDialog } from '@webapp/presentation/components/detail';
import { TrashPageActions, TrashDialogs } from '@webapp/presentation/components/trash';"#
        } else {
            r#"import { ListPageActions, DeleteDialogs } from '@webapp/presentation/components/list';
import { DangerZone, DeleteDialog } from '@webapp/presentation/components/detail';"#
        };

        let mut content = format!(
r#"/**
 * {entity_pascal} CRUD Pages
 *
 * List, Create, Edit, and Detail pages for {entity_pascal} entity.{trash_note}
 * Generated from schema definition.
 * Uses reusable hooks and components for CRUD operations.
 *
 * @module presentation/pages/{module}/{entity_pascal}Pages
 */

import React, {{ useState, useCallback }} from 'react';
import {{ useNavigate, useParams }} from 'react-router-dom';
import {{
  use{entity_pascal},
  use{entity_pascal}List,
  useCreate{entity_pascal},
  useUpdate{entity_pascal},
  useDelete{entity_pascal},
{soft_delete_imports}
}} from '@webapp/domain/{module}/service/{entity_pascal}Service';
import type {{
  {entity_pascal},
  Create{entity_pascal}Input,
  Update{entity_pascal}Input,
}} from '@webapp/domain/{module}/entity/{entity_pascal}.schema';
import {{
  {entity_pascal}CreateForm,
  {entity_pascal}EditForm,
}} from '@webapp/presentation/components/forms/{module}/{entity_pascal}FormFields';
import {{
  use{entity_pascal}TableColumns,
}} from '@webapp/presentation/components/tables/{module}/{entity_pascal}TableColumns';
{reusable_hooks_imports}
{reusable_components_imports}
import {{
  Box,
  Container,
  Stack,
  Typography,
  Button,
  Sheet,
  CircularProgress,
  Alert,
}} from '@/components/ui';
import {{ Plus{soft_delete_icons} }} from '@/components/ui';

// ============================================================================
// List Page
// ============================================================================

/**
 * {entity_pascal} list page with data table
 * Uses reusable useListOperations hook and ListPageActions component
 */
export function {entity_pascal}ListPage() {{
  const navigate = useNavigate();
  const [page, setPage] = useState(1);
  const [search, setSearch] = useState('');

  const {{ data, isLoading, error, refetch }} = use{entity_pascal}List(
    {{ page, limit: 20, search }},
    undefined
  );

  const deleteMutation = useDelete{entity_pascal}();

  // Use reusable list operations hook
  const listOps = useListOperations<{entity_pascal}>({{
    deleteMutation,
    onRefetch: refetch,
  }});

  const handleRefresh = useCallback(async () => {{
    await refetch();
  }}, [refetch]);

  const columns = use{entity_pascal}TableColumns({{
    onView: (row) => navigate(`/{module}/{entity_snake}/${{row.id}}`),
    onEdit: (row) => navigate(`/{module}/{entity_snake}/${{row.id}}/edit`),
    onDelete: (row) => listOps.handleDelete(row),
  }});

  if (error) {{
    return (
      <Container>
        <Alert color="danger">
          Error loading {entity_pascal} list: {{error.message}}
        </Alert>
      </Container>
    );
  }}

  return (
    <Container maxWidth="xl">
      <Stack spacing={{3}}>
        <Stack direction="row" justifyContent="space-between" alignItems="center">
          <Typography level="h3">{entity_pascal} List</Typography>
          <ListPageActions
            selectedCount={{listOps.selectedCount}}
            hasSelectedRows={{listOps.hasSelectedRows}}
            onBulkDelete={{listOps.handleBulkDelete}}
            onRefresh={{handleRefresh}}
            onCreate={{() => navigate('/{module}/{entity_snake}/new')}}
            isRefreshing={{isLoading}}
            isDeleting={{deleteMutation.isPending}}
            addButtonLabel="Create {entity_pascal}"{trash_menu_item}
          />
        </Stack>

        <Sheet variant="outlined" sx={{{{ p: 2 }}}}>
          <Box component="input"
            type="search"
            placeholder="Search..."
            value={{search}}
            onChange={{(e) => setSearch(e.target.value)}}
            sx={{{{
              width: '100%',
              maxWidth: '400px',
              p: 1.5,
              border: '1px solid',
              borderColor: 'neutral.outlinedBorder',
              borderRadius: 'sm',
              '&:focus': {{
                outline: 'none',
                borderColor: 'primary.outlinedBorder',
              }},
            }}}}
          />
        </Sheet>

        {{isLoading ? (
          <Box display="flex" justifyContent="center" py={{12}}>
            <CircularProgress />
          </Box>
        ) : (
          <Sheet variant="outlined" sx={{{{ overflow: 'hidden' }}}}>
            {{/* TODO: Replace with proper DataGrid component */}}
            <Box sx={{{{ p: 2 }}}}>
              <Typography level="body-sm">
                Found {{data?.total ?? 0}} {entity_snake} entities
              </Typography>
            </Box>
          </Sheet>
        )}}
      </Stack>

      {{/* Delete Dialogs */}}
      <DeleteDialogs<{entity_pascal}>
        entityName="{entity_snake}"
        deleteConfirmOpen={{listOps.deleteConfirmOpen}}
        onCloseDeleteDialog={{listOps.handleCloseDeleteDialog}}
        onConfirmDelete={{listOps.handleConfirmDelete}}
        deleteError={{listOps.deleteError}}
        isBulkOperation={{listOps.isBulkOperation}}
        itemToDelete={{listOps.itemToDelete}}
        itemsToDelete={{listOps.itemsToDelete}}
        isDeleting={{deleteMutation.isPending}}
        getItemDisplayName={{(item) => item.id}}
        bulkDeleteProgress={{listOps.bulkDeleteProgress}}{is_soft_delete_prop}
      />
    </Container>
  );
}}"#,
            entity_pascal = entity_pascal,
            entity_snake = entity_snake,
            module = self.config.module,
            soft_delete_imports = soft_delete_imports,
            soft_delete_icons = soft_delete_icons,
            reusable_hooks_imports = reusable_hooks_imports,
            reusable_components_imports = reusable_components_imports,
            trash_note = if has_soft_delete { "\n * Includes Trash page for soft-deleted entities." } else { "" },
            trash_menu_item = if has_soft_delete {
                format!(
r#"
            moreMenuItems={{[
              {{
                label: 'Trash',
                icon: <DeleteSweep />,
                onClick: () => navigate('/{module}/{entity_snake}/trash'),
              }},
            ]}}"#,
                    module = self.config.module,
                    entity_snake = entity_snake,
                )
            } else {
                String::new()
            },
            is_soft_delete_prop = if has_soft_delete {
                "\n        isSoftDelete"
            } else {
                ""
            },
        );

        content.push_str(&format!(
r#"

// ============================================================================
// Detail Page
// ============================================================================

/**
 * {entity_pascal} detail view page
 * Uses reusable useDetailOperations hook, DangerZone and DeleteDialog components
 */
export function {entity_pascal}DetailPage() {{
  const navigate = useNavigate();
  const {{ id }} = useParams<{{ id: string }}>();
  const {{ data: {entity_camel}, isLoading, error, refetch }} = use{entity_pascal}(id ?? '');
  const deleteMutation = useDelete{entity_pascal}();

  // Use reusable detail operations hook
  const detailOps = useDetailOperations({{
    entityId: id ?? '',
    deleteMutation,
    onDeleteSuccess: () => navigate('/{module}/{entity_snake}'),
    onRefetch: refetch,
  }});

  if (isLoading) {{
    return (
      <Container>
        <Box display="flex" justifyContent="center" py={{12}}>
          <CircularProgress />
        </Box>
      </Container>
    );
  }}

  if (error || !{entity_camel}) {{
    return (
      <Container>
        <Alert color="danger">
          {{error?.message ?? '{entity_pascal} not found'}}
        </Alert>
      </Container>
    );
  }}

  return (
    <Container maxWidth="xl">
      <Stack spacing={{3}}>
        <Stack direction="row" justifyContent="space-between" alignItems="center">
          <Typography level="h3">{entity_pascal} Details</Typography>
          <Stack direction="row" spacing={{1}}>
            <Button
              variant="outlined"
              onClick={{() => navigate('/{module}/{entity_snake}')}}
            >
              Back to List
            </Button>
            <Button
              variant="solid"
              onClick={{() => navigate(`/{module}/{entity_snake}/${{{entity_camel}.id}}/edit`)}}
            >
              Edit
            </Button>
          </Stack>
        </Stack>

        <Sheet variant="outlined">
          <Box sx={{{{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(250px, 1fr))', gap: 2, p: 3 }}}}>
            {{Object.entries({entity_camel}).map(([key, value]) => (
              <Box key={{key}}>
                <Typography level="body-xs" textColor="neutral" sx={{{{ mb: 0.5 }}}}>
                  {{key}}
                </Typography>
                <Typography level="body-sm">
                  {{typeof value === 'object' ? JSON.stringify(value) : String(value ?? '-')}}
                </Typography>
              </Box>
            ))}}
          </Box>
        </Sheet>

        {{/* Danger Zone */}}
        <DangerZone
          entityName="{entity_snake}"
          onDelete={{detailOps.handleDelete}}{is_soft_delete_detail}
        />
      </Stack>

      {{/* Delete Dialog */}}
      <DeleteDialog
        open={{detailOps.deleteConfirmOpen}}
        onClose={{detailOps.handleCloseDeleteDialog}}
        onConfirm={{detailOps.handleConfirmDelete}}
        entityName="{entity_snake}"
        itemDisplayName={{{entity_camel}.id}}
        error={{detailOps.deleteError}}
        isDeleting={{detailOps.isDeleting}}{is_soft_delete_detail}
      />
    </Container>
  );
}}

// ============================================================================
// Create Page
// ============================================================================

/**
 * {entity_pascal} create page
 */
export function {entity_pascal}CreatePage() {{
  const navigate = useNavigate();
  const createMutation = useCreate{entity_pascal}();

  const handleSubmit = async (data: Create{entity_pascal}Input) => {{
    const result = await createMutation.mutateAsync(data);
    navigate(`/{module}/{entity_snake}/${{result.id}}`);
  }};

  return (
    <Container maxWidth="md">
      <Stack spacing={{3}}>
        <Typography level="h3">Create {entity_pascal}</Typography>

        <Sheet variant="outlined" sx={{{{ p: 3 }}}}>
          <{entity_pascal}CreateForm
            onSubmit={{handleSubmit}}
            onCancel={{() => navigate('/{module}/{entity_snake}')}}
            isLoading={{createMutation.isPending}}
          />
          {{createMutation.isError && (
            <Alert color="danger" sx={{{{ mt: 2 }}}}>
              {{createMutation.error.message}}
            </Alert>
          )}}
        </Sheet>
      </Stack>
    </Container>
  );
}}

// ============================================================================
// Edit Page
// ============================================================================

/**
 * {entity_pascal} edit page
 */
export function {entity_pascal}EditPage() {{
  const navigate = useNavigate();
  const {{ id }} = useParams<{{ id: string }}>();
  const {{ data: {entity_camel}, isLoading, error }} = use{entity_pascal}(id ?? '');
  const updateMutation = useUpdate{entity_pascal}();

  const handleSubmit = async (data: Update{entity_pascal}Input) => {{
    await updateMutation.mutateAsync({{ id: id!, input: data }});
    navigate(`/{module}/{entity_snake}/${{id}}`);
  }};

  if (isLoading) {{
    return (
      <Container>
        <Box display="flex" justifyContent="center" py={{12}}>
          <CircularProgress />
        </Box>
      </Container>
    );
  }}

  if (error || !{entity_camel}) {{
    return (
      <Container>
        <Alert color="danger">
          {{error?.message ?? '{entity_pascal} not found'}}
        </Alert>
      </Container>
    );
  }}

  return (
    <Container maxWidth="md">
      <Stack spacing={{3}}>
        <Typography level="h3">Edit {entity_pascal}</Typography>

        <Sheet variant="outlined" sx={{{{ p: 3 }}}}>
          <{entity_pascal}EditForm
            onSubmit={{handleSubmit}}
            onCancel={{() => navigate(`/{module}/{entity_snake}/${{id}}`)}}
            defaultValues={{{entity_camel} as Update{entity_pascal}Input}}
            isLoading={{updateMutation.isPending}}
          />
          {{updateMutation.isError && (
            <Alert color="danger" sx={{{{ mt: 2 }}}}>
              {{updateMutation.error.message}}
            </Alert>
          )}}
        </Sheet>
      </Stack>
    </Container>
  );
}}
"#,
            entity_pascal = entity_pascal,
            entity_camel = entity_camel,
            entity_snake = entity_snake,
            module = self.config.module,
            is_soft_delete_detail = if has_soft_delete {
                "\n          isSoftDelete"
            } else {
                ""
            },
        ));

        // Add Trash pages for soft delete entities
        if has_soft_delete {
            content.push_str(&format!(
r#"
// ============================================================================
// Trash List Page (Soft Delete)
// ============================================================================

/**
 * {entity_pascal} trash list page - shows soft-deleted entities
 * Uses reusable useTrashOperations hook, TrashPageActions and TrashDialogs components
 */
export function {entity_pascal}TrashListPage() {{
  const navigate = useNavigate();
  const [page, setPage] = useState(1);
  const [search, setSearch] = useState('');
  const [restoreSuccess, setRestoreSuccess] = useState<string | null>(null);

  const {{ data, isLoading, error, refetch }} = use{entity_pascal}TrashList(
    {{ page, limit: 20, search }},
    undefined
  );

  const restoreMutation = use{entity_pascal}Restore();
  const permanentDeleteMutation = use{entity_pascal}PermanentDelete();

  const {entity_snake}s = data?.items ?? [];
  const total = data?.total ?? 0;

  // Use reusable trash operations hook
  const trashOps = useTrashOperations<{entity_pascal}>({{
    restoreMutation,
    permanentDeleteMutation,
    allItems: {entity_snake}s,
    onRestoreSuccess: (count) => {{
      setRestoreSuccess(`${{count}} {entity_snake}(s) restored successfully`);
      setTimeout(() => setRestoreSuccess(null), 3000);
    }},
  }});

  const handleRefresh = useCallback(async () => {{
    await refetch();
  }}, [refetch]);

  if (error) {{
    return (
      <Container>
        <Alert color="danger">
          Error loading trash: {{error.message}}
        </Alert>
      </Container>
    );
  }}

  return (
    <Container maxWidth="xl">
      <Stack spacing={{3}}>
        <Stack direction="row" justifyContent="space-between" alignItems="center">
          <Typography level="h3">{entity_pascal} Trash</Typography>
          <TrashPageActions
            selectedCount={{trashOps.selectedCount}}
            hasSelectedRows={{trashOps.hasSelectedRows}}
            totalItems={{total}}
            onBulkRestore={{trashOps.handleBulkRestore}}
            onBulkPermanentDelete={{trashOps.handleBulkPermanentDelete}}
            onEmptyTrash={{trashOps.handleEmptyTrash}}
            onRefresh={{handleRefresh}}
            isRestorePending={{restoreMutation.isPending}}
            isDeletePending={{permanentDeleteMutation.isPending}}
            isRefreshing={{isLoading}}
            isEmptyingTrash={{trashOps.isEmptyingTrash}}
          />
        </Stack>

        {{restoreSuccess && (
          <Alert color="success">{{restoreSuccess}}</Alert>
        )}}

        <Alert color="info">
          Items in trash can be restored or permanently deleted. Permanent deletion cannot be undone.
        </Alert>

        <Sheet variant="outlined" sx={{{{ p: 2 }}}}>
          <Box component="input"
            type="search"
            placeholder="Search trash..."
            value={{search}}
            onChange={{(e) => setSearch(e.target.value)}}
            sx={{{{
              width: '100%',
              maxWidth: '400px',
              p: 1.5,
              border: '1px solid',
              borderColor: 'neutral.outlinedBorder',
              borderRadius: 'sm',
              '&:focus': {{
                outline: 'none',
                borderColor: 'primary.outlinedBorder',
              }},
            }}}}
          />
        </Sheet>

        {{isLoading ? (
          <Box display="flex" justifyContent="center" py={{12}}>
            <CircularProgress />
          </Box>
        ) : (
          <Sheet variant="outlined" sx={{{{ overflow: 'hidden' }}}}>
            {{/* TODO: Replace with proper DataGrid component with restore/permanent delete actions */}}
            <Box sx={{{{ p: 2 }}}}>
              <Typography level="body-sm">
                Found {{total}} deleted {entity_snake} entities
              </Typography>
            </Box>
          </Sheet>
        )}}
      </Stack>

      {{/* Trash Dialogs */}}
      <TrashDialogs<{entity_pascal}>
        entityName="{entity_snake}"
        permanentDeleteConfirmOpen={{trashOps.permanentDeleteConfirmOpen}}
        onClosePermanentDeleteDialog={{trashOps.handleClosePermanentDeleteDialog}}
        onConfirmPermanentDelete={{trashOps.handleConfirmPermanentDelete}}
        permanentDeleteError={{trashOps.permanentDeleteError}}
        isBulkOperation={{trashOps.isBulkOperation}}
        itemToDelete={{trashOps.itemToDelete}}
        itemsToDelete={{trashOps.itemsToDelete}}
        isDeletePending={{permanentDeleteMutation.isPending}}
        getItemDisplayName={{(item) => item.id}}
        emptyTrashConfirmOpen={{trashOps.emptyTrashConfirmOpen}}
        onCloseEmptyTrashDialog={{trashOps.handleCloseEmptyTrashDialog}}
        onConfirmEmptyTrash={{trashOps.handleConfirmEmptyTrash}}
        emptyTrashError={{trashOps.emptyTrashError}}
        isEmptyingTrash={{trashOps.isEmptyingTrash}}
        totalItems={{total}}
      />
    </Container>
  );
}}

// ============================================================================
// Trash Detail Page (Soft Delete)
// ============================================================================

/**
 * {entity_pascal} trash detail page - shows a soft-deleted entity
 * Note: Detail page for trash uses simpler direct state management
 */
export function {entity_pascal}TrashDetailPage() {{
  const navigate = useNavigate();
  const {{ id }} = useParams<{{ id: string }}>();
  const {{ data: {entity_camel}, isLoading, error }} = use{entity_pascal}(id ?? '');
  const restoreMutation = use{entity_pascal}Restore();
  const permanentDeleteMutation = use{entity_pascal}PermanentDelete();
  const [restoreDialogOpen, setRestoreDialogOpen] = useState(false);
  const [deleteDialogOpen, setDeleteDialogOpen] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);

  const handleRestoreConfirm = async () => {{
    if (!{entity_camel}) return;
    setActionError(null);
    try {{
      await restoreMutation.mutateAsync({entity_camel}.id);
      setRestoreDialogOpen(false);
      navigate('/{module}/{entity_snake}');
    }} catch (err) {{
      setActionError(err instanceof Error ? err.message : 'Failed to restore');
    }}
  }};

  const handlePermanentDeleteConfirm = async () => {{
    if (!{entity_camel}) return;
    setActionError(null);
    try {{
      await permanentDeleteMutation.mutateAsync({entity_camel}.id);
      setDeleteDialogOpen(false);
      navigate('/{module}/{entity_snake}/trash');
    }} catch (err) {{
      setActionError(err instanceof Error ? err.message : 'Failed to permanently delete');
    }}
  }};

  if (isLoading) {{
    return (
      <Container>
        <Box display="flex" justifyContent="center" py={{12}}>
          <CircularProgress />
        </Box>
      </Container>
    );
  }}

  if (error || !{entity_camel}) {{
    return (
      <Container>
        <Alert color="danger">
          {{error?.message ?? '{entity_pascal} not found in trash'}}
        </Alert>
      </Container>
    );
  }}

  return (
    <Container maxWidth="xl">
      <Stack spacing={{3}}>
        <Stack direction="row" justifyContent="space-between" alignItems="center">
          <Typography level="h3">{entity_pascal} (Deleted)</Typography>
          <Stack direction="row" spacing={{1}}>
            <Button
              variant="outlined"
              onClick={{() => navigate('/{module}/{entity_snake}/trash')}}
            >
              Back to Trash
            </Button>
            <Button
              variant="solid"
              color="success"
              startDecorator={{<RestoreFromTrash />}}
              onClick={{() => setRestoreDialogOpen(true)}}
              disabled={{restoreMutation.isPending}}
            >
              {{restoreMutation.isPending ? 'Restoring...' : 'Restore'}}
            </Button>
            <Button
              variant="solid"
              color="danger"
              onClick={{() => setDeleteDialogOpen(true)}}
              disabled={{permanentDeleteMutation.isPending}}
            >
              Delete Permanently
            </Button>
          </Stack>
        </Stack>

        <Alert color="warning">
          This item is in the trash. Restore it to make it active again, or delete it permanently.
        </Alert>

        <Sheet variant="outlined">
          <Box sx={{{{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(250px, 1fr))', gap: 2, p: 3 }}}}>
            {{Object.entries({entity_camel}).map(([key, value]) => (
              <Box key={{key}}>
                <Typography level="body-xs" textColor="neutral" sx={{{{ mb: 0.5 }}}}>
                  {{key}}
                </Typography>
                <Typography level="body-sm">
                  {{typeof value === 'object' ? JSON.stringify(value) : String(value ?? '-')}}
                </Typography>
              </Box>
            ))}}
          </Box>
        </Sheet>

        {{/* Danger Zone for permanent delete */}}
        <DangerZone
          entityName="{entity_snake}"
          onDelete={{() => setDeleteDialogOpen(true)}}
          isSoftDelete={{false}}
          description="This action is irreversible. The data will be permanently removed."
          buttonLabel="Delete Permanently"
        />
      </Stack>

      {{/* Restore Dialog - using simple custom dialog */}}
      <DeleteDialog
        open={{restoreDialogOpen}}
        onClose={{() => !restoreMutation.isPending && setRestoreDialogOpen(false)}}
        onConfirm={{handleRestoreConfirm}}
        entityName="{entity_snake}"
        itemDisplayName={{{entity_camel}.id}}
        error={{actionError}}
        isDeleting={{restoreMutation.isPending}}
        isSoftDelete={{false}}
      />

      {{/* Permanent Delete Dialog */}}
      <DeleteDialog
        open={{deleteDialogOpen}}
        onClose={{() => !permanentDeleteMutation.isPending && setDeleteDialogOpen(false)}}
        onConfirm={{handlePermanentDeleteConfirm}}
        entityName="{entity_snake}"
        itemDisplayName={{{entity_camel}.id}}
        error={{actionError}}
        isDeleting={{permanentDeleteMutation.isPending}}
        isSoftDelete={{false}}
      />
    </Container>
  );
}}
"#,
                entity_pascal = entity_pascal,
                entity_camel = entity_camel,
                entity_snake = entity_snake,
                module = self.config.module,
            ));
        }

        // Add custom section
        content.push_str(r#"
// <<< CUSTOM: Add custom page components here
// END CUSTOM
"#);

        content
    }
}
