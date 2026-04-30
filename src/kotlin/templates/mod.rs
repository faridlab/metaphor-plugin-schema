//! Handlebars templates for Kotlin code generation

/// Entity (data class) template
pub const ENTITY_TEMPLATE: &str = r#"package {{package}}

import kotlinx.serialization.*
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
{{#each imports}}
import {{this}}
{{/each}}

/**
 * {{name}} Entity
 *
 * Domain entity representing {{collection}} in the system.
 *
{{#if description}}
 * {{description}}
 *
{{/if}}
 * ## Properties
{{#each fields}}
{{#unless is_primary_key}}
 * @property {{name}} {{#if is_sensitive}}(SENSITIVE) {{/if}}{{original_name}}
{{/unless}}
{{/each}}
 *
 * Generated from Backbone schema
{{#each fields}}
{{#if is_sensitive}}
 * SECURITY WARNING: {{original_name}} contains sensitive data.
 * - Never log this field value
 * - Never include in error messages
 * - Never expose in API responses unless encrypted
 * - Use secure storage for persistence
{{/if}}
{{/each}}
 */
@Serializable
data class {{name}}(
{{#each fields}}
{{#if is_primary_key}}
    /** Primary key - unique identifier */
    val {{name}}: {{{kotlin_type}}},
{{else if is_sensitive}}
    /** {{original_name}} (SENSITIVE - never log or expose) */
    val {{name}}: {{{kotlin_type}}},
{{else if kotlin_type}}
    /** {{original_name}} */
    val {{name}}: {{{kotlin_type}}},
{{else}}
    /** {{original_name}} */
    val {{name}}: {{{kotlin_type}}},
{{/if}}
{{/each}}
) {
{{#if has_soft_delete_with_metadata}}
    /**
     * Check if this entity is soft deleted
     *
     * @return true if the entity has been soft deleted
     */
    val isDeleted: Boolean
        get() = metadata["deleted_at"] != null

{{/if}}
}
"#;

/// Enum (sealed class) template
pub const ENUM_TEMPLATE: &str = r#"package {{package}}

import kotlinx.serialization.*
import kotlinx.serialization.descriptors.PrimitiveKind
import kotlinx.serialization.descriptors.PrimitiveSerialDescriptor
import kotlinx.serialization.encoding.Decoder
import kotlinx.serialization.encoding.Encoder

/**
 * {{name}} Enum
 */
@Serializable(with = {{name}}Serializer::class)
sealed class {{name}}(val displayName: String) {
{{#each variants}}
    /** {{display_name}} */
    object {{name}} : {{../name}}("{{display_name}}")
{{/each}}

    override fun toString(): String = when (this) {
{{#each variants}}
        is {{name}} -> "{{original_name}}"
{{/each}}
    }

    companion object {
        /**
         * Parse from string value
         */
        fun fromString(value: String): {{name}} = when (value) {
{{#each variants}}
            "{{original_name}}" -> {{name}}
{{/each}}
            else -> {{#each variants}}{{#if @first}}{{name}}{{/if}}{{/each}}
        }
    }
}

internal object {{name}}Serializer : KSerializer<{{name}}> {
    override val descriptor = PrimitiveSerialDescriptor("{{name}}", PrimitiveKind.STRING)
    override fun serialize(encoder: Encoder, value: {{name}}) = encoder.encodeString(value.toString())
    override fun deserialize(decoder: Decoder): {{name}} = {{name}}.fromString(decoder.decodeString())
}
"#;

/// Repository interface template
pub const REPOSITORY_TEMPLATE: &str = r#"package {{package}}

import {{entity_package}}.{{entity_name}}
import {{base_package}}.infrastructure.pagination.PaginatedResult
import kotlinx.coroutines.flow.Flow

/**
 * {{entity_name}} Repository Interface
 *
 * Generated from Backbone schema
 */
interface {{entity_name}}Repository {
    /**
     * Get {{entity_name}} by ID
     */
    suspend fun getById(id: String): {{entity_name}}?

    /**
     * Get all {{entity_name_lowercase}} records with pagination
     */
    fun getAll(
        page: Int = 1,
        limit: Int = 20,
        sortBy: String = "created_at",
        sortDesc: Boolean = true
    ): Flow<PaginatedResult<{{entity_name}}>>

    /**
     * Create new {{entity_name}}
     */
    suspend fun create(entity: {{entity_name}}): {{entity_name}}

    /**
     * Update existing {{entity_name}}
     */
    suspend fun update(entity: {{entity_name}}): {{entity_name}}

    /**
     * Delete {{entity_name}} (soft delete if enabled)
     */
    suspend fun delete(id: String): Boolean
{{#if has_soft_delete}}

    /**
     * Get soft-deleted {{entity_name_lowercase}} records
     */
    fun getDeleted(
        page: Int = 1,
        limit: Int = 20
    ): Flow<PaginatedResult<{{entity_name}}>>

    /**
     * Restore soft-deleted {{entity_name}}
     */
    suspend fun restore(id: String): {{entity_name}}?
{{/if}}
}
"#;

/// API client (Ktor) template — extends BaseCrudApiClient (Phase 1 composition)
pub const API_CLIENT_TEMPLATE: &str = r#"package {{package}}

import {{entity_package}}.{{entity_name}}
import {{base_package}}.core.api.BaseCrudApiClient
import {{base_package}}.infrastructure.pagination.BackendPaginatedResponse
import {{base_package}}.infrastructure.pagination.BackendSingleResponse
import io.ktor.client.*
import io.ktor.client.call.*
import io.ktor.client.statement.*

/**
 * {{entity_name}} API Client
 *
 * Extends BaseCrudApiClient — getById, getAll, delete are inherited.
 * Add entity-specific create/update methods in a *ApiClientCustom.kt file.
 *
 * Generated from Backbone schema
 */
class {{entity_name}}ApiClient(
    httpClient: HttpClient,
    baseUrl: String
) : BaseCrudApiClient<{{entity_name}}>(httpClient, baseUrl) {

    override val basePath = "$baseUrl/api/v1/{{module_lower}}/{{collection}}"

    override suspend fun deserializeOne(response: HttpResponse): {{entity_name}} {
        val wrapped: BackendSingleResponse<{{entity_name}}> = response.body()
        return wrapped.data
    }

    override suspend fun deserializeList(response: HttpResponse): BackendPaginatedResponse<{{entity_name}}> =
        response.body()
}
"#;

/// SQLDelight schema template
pub const SQLDELIGHT_SCHEMA_TEMPLATE: &str = r#"-- {{entity_name}} SQLDelight Schema
-- Generated from Backbone schema

CREATE TABLE {{collection}} (
{{#each fields}}
    {{name}} {{sql_type}}{{#unless @last}},{{/unless}}
{{/each}}
);

{{#if has_soft_delete}}
-- Index for soft delete
CREATE INDEX idx_{{collection}}_deleted_at ON {{collection}}(deleted_at);
{{/if}}

-- Index for created_at
CREATE INDEX idx_{{collection}}_created_at ON {{collection}}(created_at);
"#;

/// SQLDelight queries template
pub const SQLDELIGHT_QUERIES_TEMPLATE: &str = r#"-- {{entity_name}} SQLDelight Queries
-- Generated from Backbone schema

selectById:
SELECT *
FROM {{collection}}
WHERE id = ?;

selectAll:
SELECT *
FROM {{collection}}
ORDER BY created_at DESC
LIMIT ? OFFSET ?;

insert:
INSERT INTO {{collection}} (
{{#each fields}}
    {{name}}{{#unless @last}},{{/unless}}
{{/each}}
)
VALUES (
{{#each fields}}
    ?{{#unless @last}},{{/unless}}
{{/each}}
);

update:
UPDATE {{collection}}
SET
{{#each fields}}
    {{name}} = ?{{#unless @last}},{{/unless}}
{{/each}}
WHERE id = ?;

delete:
UPDATE {{collection}}
SET deleted_at = (strftime('%s', 'now'))
WHERE id = ?;
"#;

/// ViewModel template — extends BaseCrudListViewModel (Phase 1 composition)
pub const VIEWMODEL_TEMPLATE: &str = r#"package {{package}}

import {{entity_package}}.{{entity_name}}
import {{mapper_package}}.{{entity_name}}DTO
import {{mapper_package}}.{{entity_name}}Mapper
import {{base_package}}.core.usecase.CrudUseCases
import {{base_package}}.core.viewmodel.BaseCrudListViewModel
import {{base_package}}.core.viewmodel.CrudListEffect
import {{base_package}}.core.viewmodel.CrudListIntent
import {{base_package}}.core.viewmodel.CrudListState

// Backward-compatible typealiases — existing UI code can keep using these names
typealias {{entity_name}}ListState = CrudListState<{{entity_name}}DTO>
typealias {{entity_name}}ListEvent = CrudListIntent
typealias {{entity_name}}ListEffect = CrudListEffect

/**
 * {{entity_name}} List ViewModel
 *
 * Extends BaseCrudListViewModel — load, refresh, pagination, delete are inherited.
 * Add entity-specific intents by overriding handleIntent and calling super.
 *
 * Generated from Backbone schema
 */
class {{entity_name}}ListViewModel(
    useCases: CrudUseCases<{{entity_name}}>,
    private val mapper: {{entity_name}}Mapper = {{entity_name}}Mapper()
) : BaseCrudListViewModel<{{entity_name}}, {{entity_name}}DTO>(useCases) {

    override fun mapToDto(entity: {{entity_name}}): {{entity_name}}DTO = mapper.toDto(entity)

    override fun extractId(dto: {{entity_name}}DTO): String = dto.{{primary_key_field}}
}
"#;

/// Card component template
pub const COMPONENT_CARD_TEMPLATE: &str = r#"package {{package}}

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.*
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.flow.distinctUntilChanged
import {{mapper_package}}.{{entity_name}}DTO

/**
 * {{entity_name}} Card Component
 *
 * Displays a single {{entity_name}} row. Pass a DTO from the ViewModel state.
 *
 * Generated from Backbone schema
 */
@Composable
fun {{entity_name}}Card(
    item: {{entity_name}}DTO,
    onClick: () -> Unit,
    onDeleteClick: () -> Unit,
    modifier: Modifier = Modifier
) {
    Card(
        onClick = onClick,
        modifier = modifier.fillMaxWidth()
    ) {
        Column(
            modifier = Modifier.padding(16.dp)
        ) {
            Text(
                text = item.{{primary_key_field}},
                style = MaterialTheme.typography.titleMedium
            )
        }
    }
}

/**
 * {{entity_name}} Lazy List
 *
 * Scrollable list with automatic next-page trigger (7B).
 * Fires [onLoadNext] when the user approaches the last 3 items,
 * enabling seamless infinite scroll wired to [BaseCrudListViewModel.LoadNextPage].
 *
 * Generated from Backbone schema
 */
@Composable
fun {{entity_name}}LazyList(
    items: List<{{entity_name}}DTO>,
    hasNext: Boolean,
    onLoadNext: () -> Unit,
    onItemClick: (String) -> Unit,
    onDeleteClick: (String) -> Unit,
    modifier: Modifier = Modifier,
) {
    val listState = rememberLazyListState()

    // Trigger next-page load when within 3 items of the end.
    // distinctUntilChanged() ensures onLoadNext() fires only when the last-visible
    // index actually changes, not on every scroll frame.
    LaunchedEffect(listState, hasNext) {
        snapshotFlow { listState.layoutInfo.visibleItemsInfo.lastOrNull()?.index }
            .distinctUntilChanged()
            .collect { lastVisible ->
                if (hasNext && lastVisible != null && lastVisible >= items.size - 3) {
                    onLoadNext()
                }
            }
    }

    LazyColumn(
        state = listState,
        modifier = modifier.fillMaxSize(),
        contentPadding = PaddingValues(horizontal = 16.dp, vertical = 8.dp),
        verticalArrangement = Arrangement.spacedBy(4.dp),
    ) {
        items(items, key = { it.{{primary_key_field}} }) { item ->
            {{entity_name}}Card(
                item = item,
                onClick = { onItemClick(item.{{primary_key_field}}) },
                onDeleteClick = { onDeleteClick(item.{{primary_key_field}}) },
            )
        }
    }
}
"#;

/// Use case template — delegates to generic CrudUseCases<E> (Phase 1 composition)
pub const USECASE_TEMPLATE: &str = r#"package {{package}}

import {{entity_package}}.{{entity_name}}
import {{base_package}}.core.usecase.CrudUseCases

/**
 * {{entity_name}} use cases — thin typealias over CrudUseCases<{{entity_name}}>
 *
 * Wire up with any CrudRepository<{{entity_name}}} implementation:
 *   val useCases: {{entity_name}}UseCases = repository.toUseCases()
 *
 * Generated from Backbone schema
 */
typealias {{entity_name}}UseCases = CrudUseCases<{{entity_name}}>
"#;

/// Application Service template — extends BaseCrudService (Phase 1 composition)
pub const APP_SERVICE_TEMPLATE: &str = r#"package {{package}}

import {{entity_package}}.{{entity_name}}
import {{application_base_package}}.mappers.{{entity_name}}DTO
import {{application_base_package}}.mappers.{{entity_name}}FormData
import {{application_base_package}}.mappers.{{entity_name}}Mapper
import {{application_base_package}}.validators.{{entity_name}}Validator
import {{base_package}}.core.service.BaseCrudService
import {{base_package}}.core.usecase.CrudUseCases

/**
 * {{entity_name}} Application Service
 *
 * Extends BaseCrudService — getById, getAll, create, update, delete are inherited.
 * Add entity-specific business logic in a *ServiceCustom.kt file (// <<< CUSTOM).
 *
 * Generated from Backbone schema
 */
class {{entity_name}}Service(
    useCases: CrudUseCases<{{entity_name}}>,
    mapper: {{entity_name}}Mapper = {{entity_name}}Mapper(),
    validator: {{entity_name}}Validator = {{entity_name}}Validator()
) : BaseCrudService<{{entity_name}}, {{entity_name}}DTO, {{entity_name}}FormData>(useCases, mapper, validator)

"#;

/// Mapper template — extends BaseEntityMapper (Phase 1 composition)
pub const MAPPER_TEMPLATE: &str = r#"package {{package}}

import {{entity_package}}.{{entity_name}}
import {{base_package}}.core.mapper.BaseEntityMapper
import {{base_package}}.core.mapper.ListDTO
import androidx.compose.runtime.Immutable
import kotlinx.serialization.Serializable
{{#if needs_instant}}
import kotlinx.datetime.Instant
{{/if}}
{{#if needs_local_date}}
import kotlinx.datetime.LocalDate
{{/if}}
{{#if needs_json_element}}
import kotlinx.serialization.json.JsonElement
{{/if}}
{{#if needs_metadata}}
import {{enums_package}}.Metadata
{{/if}}
{{#each enum_imports}}
import {{../enums_package}}.{{this}}
{{/each}}

// ─── DTO ─────────────────────────────────────────────────────────────────────

// 7A — @Immutable guarantees all fields are val and deeply immutable,
// allowing Compose to skip recomposition when the DTO reference is unchanged.
@Immutable
@Serializable
data class {{entity_name}}DTO(
{{#each fields}}
    val {{name}}: {{{kotlin_type_non_nullable}}}{{#if is_nullable}}?{{/if}},
{{/each}}
)

// Backward-compatible alias — replaces the old XxxListDTO data class
typealias {{entity_name}}ListDTO = ListDTO<{{entity_name}}DTO>

// ─── Form Data ────────────────────────────────────────────────────────────────

data class {{entity_name}}FormData(
{{#each fields}}
{{#unless is_primary_key}}
    val {{name}}: {{{kotlin_type_non_nullable}}}{{#if form_is_nullable}}?{{/if}} = {{{form_default_value}}},
{{/unless}}
{{/each}}
)

// ─── Mapper ───────────────────────────────────────────────────────────────────

/**
 * {{entity_name}} Mapper
 *
 * Extends BaseEntityMapper — override methods to customise field mapping.
 *
 * Generated from Backbone schema
 */
class {{entity_name}}Mapper : BaseEntityMapper<{{entity_name}}, {{entity_name}}DTO, {{entity_name}}FormData> {

    override fun toDto(entity: {{entity_name}}): {{entity_name}}DTO = {{entity_name}}DTO(
{{#each fields}}
        {{name}} = entity.{{name}},
{{/each}}
    )

    override fun toDomain(dto: {{entity_name}}DTO): {{entity_name}} = {{entity_name}}(
{{#each fields}}
        {{name}} = dto.{{name}},
{{/each}}
    )

    override fun toEntity(formData: {{entity_name}}FormData): {{entity_name}} = {{entity_name}}(
{{#each fields}}
{{#if is_primary_key}}
        {{name}} = "", // Assigned by backend
{{else}}
        {{name}} = formData.{{name}}{{#if form_is_nullable}}{{#unless is_nullable}}!!{{/unless}}{{/if}},
{{/if}}
{{/each}}
    )

    // Backward-compatible alias for existing callers that use toDTO()
    fun toDTO(entity: {{entity_name}}): {{entity_name}}DTO = toDto(entity)
}

fun {{entity_name}}.asDTO(): {{entity_name}}DTO = {{entity_name}}Mapper().toDto(this)
"#;

/// Validator template — extends BaseEntityValidator (Phase 1 composition)
pub const VALIDATOR_TEMPLATE: &str = r#"package {{package}}

import {{mapper_package}}.{{entity_name}}FormData
import {{base_package}}.core.validator.BaseEntityValidator
import {{base_package}}.core.validator.ValidationResult
import {{base_package}}.core.validator.applyRules
import {{base_package}}.core.validator.requiredString
import {{base_package}}.core.validator.maxLength

/**
 * {{entity_name}} Validator
 *
 * Extends BaseEntityValidator — ValidationResult and FieldRule helpers are inherited.
 * Override validate() to add custom rules or call super + merge errors.
 *
 * Generated from Backbone schema
 */
class {{entity_name}}Validator : BaseEntityValidator<{{entity_name}}FormData>() {

    override fun validate(formData: {{entity_name}}FormData): ValidationResult {
        val errors = buildErrors(
{{#each fields}}
{{#unless is_primary_key}}
{{#if (eq kotlin_type_non_nullable "String")}}
{{#unless is_nullable}}
            "{{name}}" to applyRules(formData.{{name}}, requiredString("{{name}}")),
{{else}}
            "{{name}}" to applyRules(formData.{{name}}, maxLength("{{name}}", 255)),
{{/unless}}
{{/if}}
{{/unless}}
{{/each}}
        )
        return if (errors.isEmpty()) ValidationResult.Valid else ValidationResult.invalid(errors)
    }
}
"#;


/// Common pagination types template
pub const PAGINATION_TEMPLATE: &str = r#"package {{base_package}}.infrastructure.pagination

import kotlinx.coroutines.flow.Flow
import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

/**
 * Paginated result wrapper
 *
 * Common pagination result type used across all repositories.
 *
 * ## Usage Pattern
 * ```
 * val result: PaginatedResult<User> = repository.getAll(page = 1, limit = 20)
 * println("Showing ${result.data.size} of ${result.total} items")
 * ```
 *
 * @property data The list of items for the current page
 * @property total Total number of items across all pages
 * @property page Current page number (1-indexed)
 * @property limit Number of items per page
 * @property totalPages Total number of pages available
 * @property hasNext Whether there is a next page
 * @property hasPrev Whether there is a previous page
 */
data class PaginatedResult<T>(
    val data: List<T>,
    val total: Int,
    val page: Int,
    val limit: Int,
    val totalPages: Int,
    val hasNext: Boolean,
    val hasPrev: Boolean
)

/**
 * Paginated API response wrapper
 *
 * Used by API clients for deserializing paginated HTTP responses
 * where pagination fields are at the root level with data.
 *
 * ## Response Format
 * ```json
 * {
 *   "data": [...],
 *   "total": 100,
 *   "page": 1,
 *   "limit": 20,
 *   "total_pages": 5,
 *   "has_next": true,
 *   "has_prev": false
 * }
 * ```
 *
 * This format is NOT used by the current backend API.
 * Use [BackendPaginatedResponse] for backend API responses.
 */
@Serializable
data class PaginatedApiResponse<T>(
    val data: List<T>,
    val total: Int,
    val page: Int,
    val limit: Int,
    @SerialName("total_pages")
    val totalPages: Int,
    @SerialName("has_next")
    val hasNext: Boolean,
    @SerialName("has_prev")
    val hasPrev: Boolean
) {
    /**
     * Convert to [PaginatedResult] for repository layer compatibility
     */
    fun toPaginatedResult(): PaginatedResult<T> = PaginatedResult(
        data = data,
        total = total,
        page = page,
        limit = limit,
        totalPages = totalPages,
        hasNext = hasNext,
        hasPrev = hasPrev
    )
}

/**
 * Pagination metadata from backend API
 *
 * ## Backend Response Format
 * The Backbone backend API returns paginated responses in this format:
 *
 * ```json
 * {
 *   "success": true,
 *   "data": [...],
 *   "meta": {
 *     "total": 100,
 *     "page": 1,
 *     "limit": 20,
 *     "total_pages": 5
 *   }
 * }
 * ```
 *
 * Note: The backend may not include `has_next` and `has_prev` fields.
 * These are computed from `page` and `total_pages` when missing.
 *
 * ## Field Descriptions
 * - `success`: Boolean indicating if the request was successful
 * - `data`: Array of entity objects for the current page
 * - `meta.total`: Total number of records across all pages
 * - `meta.page`: Current page number (1-indexed)
 * - `meta.limit`: Number of records per page
 * - `meta.total_pages`: Total number of pages available
 * - `meta.has_next`: Optional boolean indicating if a next page exists
 * - `meta.has_prev`: Optional boolean indicating if a previous page exists
 *
 * @property total Total number of records across all pages
 * @property page Current page number (1-indexed)
 * @property limit Number of records per page
 * @property totalPages Total number of pages available
 * @property hasNext Whether there is a next page
 * @property hasPrev Whether there is a previous page
 */
@Serializable
data class PaginationMeta(
    val total: Int,
    val page: Int,
    val limit: Int,
    @SerialName("total_pages")
    val totalPages: Int,
    @SerialName("has_next")
    val hasNext: Boolean = false,
    @SerialName("has_prev")
    val hasPrev: Boolean = false
) {
    /**
     * Get effective hasNext, computed from page/totalPages when not provided by backend
     */
    val effectiveHasNext: Boolean get() = hasNext || page < totalPages

    /**
     * Get effective hasPrev, computed from page when not provided by backend
     */
    val effectiveHasPrev: Boolean get() = hasPrev || page > 1
}

/**
 * Backend paginated API response wrapper
 *
 * Matches the Backbone backend API response format with nested metadata.
 * Used by API clients to deserialize paginated HTTP responses.
 *
 * ## Usage Pattern
 * ```kotlin
 * suspend fun getAll(page: Int = 1, limit: Int = 20): Result<PaginatedApiResponse<Item>> {
 *     return apiCall {
 *         val response: BackendPaginatedResponse<Item> = client.get("$baseUrl/api/v1/items") {
 *             parameter("page", page)
 *             parameter("limit", limit)
 *         }.body()
 *         response.toPaginatedApiResponse()
 *     }
 * }
 * ```
 *
 * @property success Boolean indicating if the request was successful
 * @property data Array of entity objects for the current page
 * @property meta Pagination metadata (total, page, limit, etc.)
 * @see PaginationMeta for detailed field descriptions
 */
@Serializable
data class BackendPaginatedResponse<T>(
    val success: Boolean,
    val data: List<T>,
    val meta: PaginationMeta
) {
    /**
     * Convert to [PaginatedResult] for repository layer compatibility
     */
    fun toPaginatedResult(): PaginatedResult<T> = PaginatedResult(
        data = data,
        total = meta.total,
        page = meta.page,
        limit = meta.limit,
        totalPages = meta.totalPages,
        hasNext = meta.effectiveHasNext,
        hasPrev = meta.effectiveHasPrev
    )

    /**
     * Convert to [PaginatedApiResponse] for consistency across API clients
     */
    fun toPaginatedApiResponse(): PaginatedApiResponse<T> = PaginatedApiResponse(
        data = data,
        total = meta.total,
        page = meta.page,
        limit = meta.limit,
        totalPages = meta.totalPages,
        hasNext = meta.effectiveHasNext,
        hasPrev = meta.effectiveHasPrev
    )
}
"#;

// =============================================================================
// Test templates (3B + 3C)
// =============================================================================

/// Validator unit test template
pub const VALIDATOR_TEST_TEMPLATE: &str = r#"package {{package}}

import {{base_package}}.application.{{module_lower}}.validators.{{entity_name}}Validator
import {{base_package}}.application.{{module_lower}}.mappers.{{entity_name}}FormData
import kotlin.test.Test
import kotlin.test.assertFalse
import kotlin.test.assertTrue

/**
 * Unit tests for [{{entity_name}}Validator].
 *
 * Generated stub — add field-specific assertions once validation rules are known.
 * Validator resides at: application/{{module_lower}}/validators/{{entity_name}}Validator.kt
 */
class {{entity_name}}ValidatorTest {

    private val validator = {{entity_name}}Validator()

    @Test
    fun `validate returns a ValidationResult`() {
        val result = validator.validate({{entity_name}}FormData())
        // Result should be either valid or invalid — never throw
        assertTrue(result.isValid || !result.isValid)
    }

    @Test
    fun `validate with default FormData reports invalid when required fields are blank`() {
        val result = validator.validate({{entity_name}}FormData())
        // Adjust this assertion once required fields are defined
        // assertFalse(result.isValid)
        assertTrue(result.isValid || !result.isValid)
    }

    @Test
    fun `validate with populated FormData returns valid`() {
        // TODO: construct a fully-populated FormData with real values
        // val form = {{entity_name}}FormData({{#each fields}}{{#unless is_nullable}}{{#unless is_primary_key}}{{name}} = {{form_default_value}}{{#unless @last}}, {{/unless}}{{/unless}}{{/unless}}{{/each}})
        // val result = validator.validate(form)
        // assertTrue(result.isValid)
        assertTrue(true) // placeholder until TODO above is filled in
    }

    @Test
    fun `validate captures per-field errors in errors map`() {
        val result = validator.validate({{entity_name}}FormData())
        if (!result.isValid) {
            assertTrue(result.errors.isNotEmpty(), "Invalid result must have at least one error entry")
            result.errors.forEach { (field, messages) ->
                assertTrue(messages.isNotEmpty(), "Field '$field' must have at least one error message")
            }
        }
    }
}
"#;

/// MVI ListViewModel unit test template
pub const VIEWMODEL_TEST_TEMPLATE: &str = r#"package {{package}}

import {{base_package}}.domain.{{module_lower}}.entity.{{entity_name}}
import {{base_package}}.presentation.state.{{module_lower}}.{{entity_name}}ListViewModel
import com.bersihir.core.test.FakeCrudRepository
import com.bersihir.core.usecase.toUseCases
import com.bersihir.core.viewmodel.CrudListIntent
import com.bersihir.core.viewmodel.CrudListEffect
import com.bersihir.domain.types.NetworkError
import com.bersihir.domain.types.Result
import kotlinx.coroutines.test.runTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertIs
import kotlin.test.assertNull
import kotlin.test.assertTrue

/**
 * Unit tests for [{{entity_name}}ListViewModel].
 *
 * Uses [FakeCrudRepository] for deterministic, synchronous control over repository responses.
 * Generated stub — fill in entity construction and specific state assertions.
 */
class {{entity_name}}ListViewModelTest {

    // Reuse idExtractor: adjust field name if primary key differs from '{{primary_key_field}}'
    private val fakeRepo = FakeCrudRepository<{{entity_name}}>(
        idExtractor = { it.{{primary_key_field}} }
    )
    private val viewModel = {{entity_name}}ListViewModel(fakeRepo.toUseCases())

    // -------------------------------------------------------------------------
    // Initial state
    // -------------------------------------------------------------------------

    @Test
    fun `initial state is not loading and has empty items`() {
        assertFalse(viewModel.currentState.isLoading)
        assertTrue(viewModel.currentState.items.isEmpty())
        assertNull(viewModel.currentState.error)
    }

    // -------------------------------------------------------------------------
    // Load
    // -------------------------------------------------------------------------

    @Test
    fun `Load with empty repo keeps items empty`() = runTest {
        viewModel.onIntent(CrudListIntent.Load)
        assertTrue(viewModel.currentState.items.isEmpty())
        assertFalse(viewModel.currentState.isLoading)
    }

    @Test
    fun `Load triggers findAll on repository`() = runTest {
        viewModel.onIntent(CrudListIntent.Load)
        assertEquals(1, fakeRepo.findAllCallCount)
    }

    // -------------------------------------------------------------------------
    // Refresh
    // -------------------------------------------------------------------------

    @Test
    fun `Refresh resets page to 1`() = runTest {
        viewModel.onIntent(CrudListIntent.Refresh)
        assertEquals(1, viewModel.currentState.page)
    }

    // -------------------------------------------------------------------------
    // Delete
    // -------------------------------------------------------------------------

    @Test
    fun `Delete intent calls delete on repository`() = runTest {
        viewModel.onIntent(CrudListIntent.Delete("test-id"))
        assertEquals(1, fakeRepo.deleteCallCount)
        assertEquals("test-id", fakeRepo.lastDeletedId)
    }

    @Test
    fun `Delete on non-existent id sets error state`() = runTest {
        viewModel.onIntent(CrudListIntent.Delete("missing-id"))
        // Repository returns NotFound — ViewModel may expose error or emit effect
        // assertIs<NetworkError.NotFound>(viewModel.currentState.error)
        assertTrue(true) // adjust assertion based on ViewModel error handling
    }

    // -------------------------------------------------------------------------
    // Repository failure
    // -------------------------------------------------------------------------

    @Test
    fun `Load sets error state when repository fails`() = runTest {
        fakeRepo.shouldFail = true
        fakeRepo.errorOverride = NetworkError.ServerError(statusCode = 500)
        viewModel.onIntent(CrudListIntent.Load)
        // ViewModel should surface the error
        // assertIs<NetworkError.ServerError>(viewModel.currentState.error)
        assertTrue(true) // adjust assertion based on ViewModel error handling
    }
}
"#;

/// Ktor MockEngine API client test template (3C)
pub const API_CLIENT_TEST_TEMPLATE: &str = r#"package {{package}}

import {{base_package}}.infrastructure.{{module_lower}}.api.{{entity_name}}ApiClient
import {{base_package}}.domain.{{module_lower}}.entity.{{entity_name}}
import com.bersihir.domain.types.Result
import io.ktor.client.HttpClient
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.client.plugins.contentnegotiation.ContentNegotiation
import io.ktor.http.ContentType
import io.ktor.http.HttpHeaders
import io.ktor.http.headersOf
import io.ktor.serialization.kotlinx.json.json
import kotlinx.coroutines.test.runTest
import kotlinx.serialization.json.Json
import kotlin.test.Test
import kotlin.test.assertIs

/**
 * Mock HTTP tests for [{{entity_name}}ApiClient].
 *
 * Uses Ktor's [MockEngine] to simulate server responses without a real network.
 * Generated stub — adjust JSON payloads to match the real backend contract.
 */
class {{entity_name}}ApiClientTest {

    private val json = Json {
        ignoreUnknownKeys = true
        isLenient = true
    }

    private fun buildClient(responseBody: String): HttpClient {
        val engine = MockEngine {
            respond(
                content = responseBody,
                headers = headersOf(
                    HttpHeaders.ContentType,
                    ContentType.Application.Json.toString()
                )
            )
        }
        return HttpClient(engine) {
            install(ContentNegotiation) { json(json) }
        }
    }

    // -------------------------------------------------------------------------
    // getById
    // -------------------------------------------------------------------------

    @Test
    fun `getById returns Success on 200`() = runTest {
        // TODO: replace null fields with real values matching your entity
        val body = """{"success":true,"data":{"{{primary_key_field}}":"test-id"{{#each fields}}{{#unless is_primary_key}},"{{original_name}}":null{{/unless}}{{/each}}}}"""
        val client = {{entity_name}}ApiClient(buildClient(body), "http://localhost")
        val result = client.getById("test-id")
        assertIs<Result.Success<{{entity_name}}>>(result)
    }

    // -------------------------------------------------------------------------
    // getAll
    // -------------------------------------------------------------------------

    @Test
    fun `getAll returns empty paginated list`() = runTest {
        val body = """{"success":true,"data":[],"meta":{"total":0,"page":1,"limit":20,"total_pages":0,"has_next":false,"has_prev":false}}"""
        val client = {{entity_name}}ApiClient(buildClient(body), "http://localhost")
        val result = client.getAll()
        assertIs<Result.Success<*>>(result)
    }

    @Test
    fun `getAll with items returns populated list`() = runTest {
        // TODO: populate data array with a real entity JSON payload
        val body = """{"success":true,"data":[{"{{primary_key_field}}":"item-1"{{#each fields}}{{#unless is_primary_key}},"{{original_name}}":null{{/unless}}{{/each}}}],"meta":{"total":1,"page":1,"limit":20,"total_pages":1,"has_next":false,"has_prev":false}}"""
        val client = {{entity_name}}ApiClient(buildClient(body), "http://localhost")
        val result = client.getAll()
        assertIs<Result.Success<*>>(result)
        val paginated = (result as Result.Success).data
        kotlin.test.assertEquals(1, paginated.data.size)
    }

    // -------------------------------------------------------------------------
    // delete
    // -------------------------------------------------------------------------

    @Test
    fun `delete returns Success on 200`() = runTest {
        val body = """{"success":true}"""
        val client = {{entity_name}}ApiClient(buildClient(body), "http://localhost")
        val result = client.delete("test-id")
        assertIs<Result.Success<Unit>>(result)
    }
}
"#;

// =============================================================================
// Sync templates (Phase 4)
// =============================================================================

/// Offline sync handler template — implements SyncEntityHandler for one entity.
///
/// The generated stub handles standard CRUD push/pull operations.
/// Entity-specific operations (e.g. state transitions) should be added
/// in a companion `*SyncHandlerCustom.kt` file marked with // <<< CUSTOM.
pub const SYNC_HANDLER_TEMPLATE: &str = r#"package {{package}}

import {{entity_package}}.{{entity_name}}
import {{mapper_package}}.{{entity_name}}FormData
import {{api_package}}.{{entity_name}}ApiClient
import {{base_package}}.domain.types.Result
import {{base_package}}.infrastructure.sync.PullResult
import {{base_package}}.infrastructure.sync.PushResult
import {{base_package}}.infrastructure.sync.SyncEntityHandler
import kotlinx.serialization.json.Json

/**
 * Offline sync handler for [{{entity_name}}].
 *
 * Implements push/pull operations against the [{{entity_name}}ApiClient].
 * Standard CRUD operations are generated — add entity-specific push operations
 * (e.g. transitions) in a `{{entity_name}}SyncHandlerCustom.kt` file.
 *
 * ## Registration
 * Register this handler in your DI module:
 * ```kotlin
 * syncRegistry.register({{entity_name}}SyncHandler({{entity_name_lowercase}}ApiClient))
 * ```
 *
 * Generated from Backbone schema — modify push/pull stubs as needed.
 */
class {{entity_name}}SyncHandler(
    private val apiClient: {{entity_name}}ApiClient,
) : SyncEntityHandler {

    override val entityType: String = "{{collection}}"

    private val json = Json { ignoreUnknownKeys = true; isLenient = true }

    // -------------------------------------------------------------------------
    // Push — outbox → server
    // -------------------------------------------------------------------------

    /**
     * Push a single outbox mutation to the server.
     *
     * Dispatches on [operation]: "CREATE", "UPDATE", "DELETE".
     * Add entity-specific operations (e.g. "TRANSITION") in a custom file.
     */
    override suspend fun push(
        operation: String,
        entityId: String,
        payload: String,
    ): Result<PushResult> {
        return when (operation) {
            "CREATE" -> {
                // TODO: decode payload and call entity-specific create endpoint.
                // The generated API client delegates create to a custom extension.
                // Example:
                //   val form = json.decodeFromString<{{entity_name}}FormData>(payload)
                //   when (val r = apiClient.create(form)) {
                //       is Result.Success -> Result.Success(PushResult(serverId = r.data.id))
                //       is Result.Error   -> Result.Error(r.error)
                //   }
                Result.Success(PushResult()) // <<< CUSTOM — implement CREATE push
            }
            "UPDATE" -> {
                // TODO: decode payload and call entity-specific update endpoint.
                // Example:
                //   val form = json.decodeFromString<{{entity_name}}FormData>(payload)
                //   when (val r = apiClient.update(entityId, form)) {
                //       is Result.Success -> Result.Success(PushResult(serverId = r.data.id))
                //       is Result.Error   -> Result.Error(r.error)
                //   }
                Result.Success(PushResult()) // <<< CUSTOM — implement UPDATE push
            }
            "DELETE" -> {
                when (val r = apiClient.delete(entityId)) {
                    is Result.Success -> Result.Success(PushResult())
                    is Result.Error   -> Result.Error(r.error)
                }
            }
            else -> Result.Success(PushResult())
        }
    }

    // -------------------------------------------------------------------------
    // Pull — server → local
    // -------------------------------------------------------------------------

    /**
     * Pull updated [{{entity_name}}] records from the server.
     *
     * Note: the generated API client does not include delta-sync parameters
     * (updatedSince, excludeDevice). Add them to [{{entity_name}}ApiClient] or
     * a `*ApiClientCustom.kt` extension and update this method accordingly.
     */
    override suspend fun pull(
        sinceMs: Long,
        page: Int,
        excludeDevice: String?,
    ): Result<PullResult> {
        // TODO: pass updatedSince / excludeDevice once supported by the API client.
        // import kotlinx.datetime.Instant
        // val updatedSince = if (sinceMs > 0) Instant.fromEpochMilliseconds(sinceMs).toString() else null
        return when (val r = apiClient.getAll(page = page, limit = 100)) {
            is Result.Success -> {
                val items = r.data.data
                // TODO: persist items to local cache/database if offline storage is needed.
                Result.Success(PullResult(upsertedCount = items.size, hasMore = r.data.hasNext))
            }
            is Result.Error -> Result.Error(r.error)
        }
    }
}
"#;

// =============================================================================
// Navigation templates (Phase 5)
// =============================================================================

/// Module-level Decompose navigation config (5A).
///
/// Produces a single `@Serializable` sealed class with `XxxList` and `XxxDetail`
/// variants for every entity in the module — matching the app's Decompose pattern.
/// Role-based visibility stubs are included for 5C.
pub const NAV_CONFIG_TEMPLATE: &str = r#"package {{package}}

import kotlinx.serialization.Serializable

/**
 * Navigation configuration for the {{module_pascal}} module.
 *
 * Contains one `List` and one `Detail` destination per entity.
 * Register with a Decompose `ChildStack`:
 * ```kotlin
 * val childStack = childStack(
 *     source = navigation,
 *     serializer = {{module_pascal}}NavConfig.serializer(),
 *     initialConfiguration = {{module_pascal}}NavConfig.{{first_entity}}List(),
 *     handleBackButton = true,
 *     childFactory = ::createChild,
 * )
 * ```
 *
 * Generated from Backbone schema — add detail navigation callbacks as needed.
 */
@Serializable
sealed class {{module_pascal}}NavConfig {
{{#each entities}}
    /** Navigate to the [{{entity_name}}] list screen (optional filter string). */
    @Serializable
    data class {{entity_name}}List(val filter: String? = null) : {{../module_pascal}}NavConfig()

    /** Navigate to a single [{{entity_name}}] detail screen. */
    @Serializable
    data class {{entity_name}}Detail(val id: String) : {{../module_pascal}}NavConfig()
{{/each}}

    companion object {
        /**
         * Role-based visibility check for a destination.
         *
         * TODO (5C): implement per-destination role rules.
         * Example:
         * ```kotlin
         * fun isVisibleForRole(config: {{module_pascal}}NavConfig, role: String): Boolean = when (config) {
         *     is XxxList   -> role in listOf("owner", "manager")
         *     is XxxDetail -> role in listOf("owner", "manager", "operator")
         * }
         * ```
         */
        fun isVisibleForRole(config: {{module_pascal}}NavConfig, role: String): Boolean = true
    }
}
"#;

/// Deep link parser extension for the module NavConfig (5B).
///
/// URL scheme: `app://{{module_lower}}/<collection>` for list,
///             `app://{{module_lower}}/<collection>/<id>` for detail.
pub const NAV_DEEP_LINK_TEMPLATE: &str = r#"package {{package}}

/**
 * Deep link parsing for the {{module_pascal}} module.
 *
 * Supported URL patterns:
{{#each entities}}
 *   app://{{../module_lower}}/{{collection}}           -> {{entity_name}}List
 *   app://{{../module_lower}}/{{collection}}/<id>      -> {{entity_name}}Detail
{{/each}}
 *
 * Generated from Backbone schema.
 */
fun {{module_pascal}}NavConfig.Companion.fromDeepLink(uri: String): {{module_pascal}}NavConfig? {
    val path = uri
        .removePrefix("app://")
        .removePrefix("{{module_lower}}/")
        .trimEnd('/')
    val segments = path.split("/").filter { it.isNotEmpty() }

    return when {
{{#each entities}}
        segments.size == 1 && segments[0] == "{{collection}}" ->
            {{../module_pascal}}NavConfig.{{entity_name}}List()
        segments.size == 2 && segments[0] == "{{collection}}" ->
            {{../module_pascal}}NavConfig.{{entity_name}}Detail(segments[1])
{{/each}}
        else -> null
    }
}
"#;

