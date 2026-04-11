//! Theme generator (Material 3)

use crate::kotlin::error::Result;
use crate::kotlin::generators::GenerationResult;
use crate::kotlin::generators::MobileGenerator;
use crate::kotlin::generators::write_generated_file;
use crate::ast::ModuleSchema;
use std::path::Path;

/// Generate Material 3 theme files
pub fn generate_theme(
    generator: &MobileGenerator,
    _schema: &ModuleSchema,
    output_dir: &Path,
) -> Result<GenerationResult> {
    let mut result = GenerationResult::default();

    // Generate Color.kt
    let color_path = generate_colors(generator, output_dir)?;
    result.generated_files.push(color_path);

    // Generate Theme.kt
    let theme_path = generate_theme_file(generator, output_dir)?;
    result.generated_files.push(theme_path);

    // Generate Type.kt
    let type_path = generate_type(generator, output_dir)?;
    result.generated_files.push(type_path);

    // Generate Shape.kt
    let shape_path = generate_shape(generator, output_dir)?;
    result.generated_files.push(shape_path);

    Ok(result)
}

/// Generate Color.kt with material color scheme
fn generate_colors(
    generator: &MobileGenerator,
    output_dir: &Path,
) -> Result<std::path::PathBuf> {
    let base_package = &generator.package_name;
    let package_name = format!("{}.presentation.ui.theme", base_package);

    let content = format!(
        r#"package {package}

import androidx.compose.ui.graphics.Color

/**
 * Color scheme for the application
 *
 * Generated from Backbone schema
 */
object AppColors {{
    // Primary brand colors
    val Primary = Color(0xFF00BCD4)          // Cyan 600
    val OnPrimary = Color(0xFFFFFFFF)         // White
    val PrimaryContainer = Color(0xFF005058)  // Cyan 900
    val OnPrimaryContainer = Color(0xFF98F0FF) // Cyan 200

    // Secondary colors
    val Secondary = Color(0xFF4A6363)        // Grey 700
    val OnSecondary = Color(0xFFFFFFFF)       // White
    val SecondaryContainer = Color(0xFFDCE4E7)// Grey 200
    val OnSecondaryContainer = Color(0xFF191C1E) // Grey 900

    // Tertiary colors
    val Tertiary = Color(0xFF5D5B7E)         // Purple Grey 700
    val OnTertiary = Color(0xFFFFFFFF)        // White
    val TertiaryContainer = Color(0xFFE3DFF9) // Purple 100
    val OnTertiaryContainer = Color(0xFF1B192B) // Purple 900

    // Error colors
    val Error = Color(0xFFBA1A1A)            // Red 700
    val OnError = Color(0xFFFFFFFF)           // White
    val ErrorContainer = Color(0xFFFFDAD6)    // Red 100
    val OnErrorContainer = Color(0xFF410002)  // Red 900

    // Background colors
    val Background = Color(0xFFFBFCFF)       // Grey 50
    val OnBackground = Color(0xFF191C1B)      // Grey 900
    val Surface = Color(0xFFFBFCFF)           // Grey 50
    val OnSurface = Color(0xFF191C1B)          // Grey 900
    val SurfaceVariant = Color(0xFFDDE2E6)    // Grey 200
    val OnSurfaceVariant = Color(0xFF41484D)  // Grey 700

    // Outline colors
    val Outline = Color(0xFF70797E)           // Grey 500
    val OutlineVariant = Color(0xFFC4C7C5)    // Grey 300

    // Custom brand colors (override as needed)
    val BrandPrimary = Color(0xFF2196F3)      // Blue 500
    val BrandSecondary = Color(0xFFFF9800)     // Orange 500
    val Success = Color(0xFF4CAF50)           // Green 500
    val Warning = Color(0xFFFFC107)           // Amber 500
    val Info = Color(0xFF2196F3)              // Blue 500
}}

/**
 * Dark color scheme
 */
object DarkAppColors {{
    val Primary = Color(0xFF98F0FF)           // Cyan 200
    val OnPrimary = Color(0xFF00373B)         // Cyan 950
    val PrimaryContainer = Color(0xFF004F57)  // Cyan 900
    val OnPrimaryContainer = Color(0xFFB4ECFF) // Cyan 100

    val Secondary = Color(0xFFBCC8CB)         // Grey 300
    val OnSecondary = Color(0xFF2F3033)        // Grey 800
    val SecondaryContainer = Color(0xFF464F52) // Grey 700
    val OnSecondaryContainer = Color(0xFFDCE4E7)// Grey 200

    val Tertiary = Color(0xFFC7C7E9)          // Purple 200
    val OnTertiary = Color(0xFF303147)        // Purple 900
    val TertiaryContainer = Color(0xFF444259)  // Purple Grey 800
    val OnTertiaryContainer = Color(0xFFE3DFF9) // Purple 100

    val Error = Color(0xFFFFB4AB)             // Red 200
    val OnError = Color(0xFF690005)           // Red 900
    val ErrorContainer = Color(0xFF93000A)    // Red 800
    val OnErrorContainer = Color(0xFFFFDAD6)   // Red 100

    val Background = Color(0xFF191C1B)        // Grey 900
    val OnBackground = Color(0xFFE1E2E5)       // Grey 200
    val Surface = Color(0xFF191C1B)            // Grey 900
    val OnSurface = Color(0xFFE1E2E5)          // Grey 200
    val SurfaceVariant = Color(0xFF41484D)     // Grey 700
    val OnSurfaceVariant = Color(0xFFC4C7C5)   // Grey 300

    val Outline = Color(0xFF8A938F)            // Grey 400
    val OutlineVariant = Color(0xFF41484D)     // Grey 700
}}
"#,
        package = package_name,
    );

    let relative_path = "presentation/ui/theme/Color.kt";
    match write_generated_file(output_dir, base_package, relative_path, &content, generator.skip_existing)? {
        crate::kotlin::generators::WriteOutcome::Written(p) | crate::kotlin::generators::WriteOutcome::Skipped(p) => Ok(p),
    }
}

/// Generate Theme.kt with Material 3 theme
fn generate_theme_file(
    generator: &MobileGenerator,
    output_dir: &Path,
) -> Result<std::path::PathBuf> {
    let base_package = &generator.package_name;
    let package_name = format!("{}.presentation.ui.theme", base_package);

    let content = format!(
        r#"package {package}

import android.app.Activity
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color

/**
 * Material 3 theme for the application
 *
 * Generated from Backbone schema
 */
private val LightColors = lightColorScheme(
    primary = AppColors.Primary,
    onPrimary = AppColors.OnPrimary,
    primaryContainer = AppColors.PrimaryContainer,
    onPrimaryContainer = AppColors.OnPrimaryContainer,
    secondary = AppColors.Secondary,
    onSecondary = AppColors.OnSecondary,
    secondaryContainer = AppColors.SecondaryContainer,
    onSecondaryContainer = AppColors.OnSecondaryContainer,
    tertiary = AppColors.Tertiary,
    onTertiary = AppColors.OnTertiary,
    tertiaryContainer = AppColors.TertiaryContainer,
    onTertiaryContainer = AppColors.OnTertiaryContainer,
    error = AppColors.Error,
    onError = AppColors.OnError,
    errorContainer = AppColors.ErrorContainer,
    onErrorContainer = AppColors.OnErrorContainer,
    background = AppColors.Background,
    onBackground = AppColors.OnBackground,
    surface = AppColors.Surface,
    onSurface = AppColors.OnSurface,
    surfaceVariant = AppColors.SurfaceVariant,
    onSurfaceVariant = AppColors.OnSurfaceVariant,
    outline = AppColors.Outline,
    outlineVariant = AppColors.OutlineVariant,
)

private val DarkColors = darkColorScheme(
    primary = DarkAppColors.Primary,
    onPrimary = DarkAppColors.OnPrimary,
    primaryContainer = DarkAppColors.PrimaryContainer,
    onPrimaryContainer = DarkAppColors.OnPrimaryContainer,
    secondary = DarkAppColors.Secondary,
    onSecondary = DarkAppColors.OnSecondary,
    secondaryContainer = DarkAppColors.SecondaryContainer,
    onSecondaryContainer = DarkAppColors.OnSecondaryContainer,
    tertiary = DarkAppColors.Tertiary,
    onTertiary = DarkAppColors.OnTertiary,
    tertiaryContainer = DarkAppColors.TertiaryContainer,
    onTertiaryContainer = DarkAppColors.OnTertiaryContainer,
    error = DarkAppColors.Error,
    onError = DarkAppColors.OnError,
    errorContainer = DarkAppColors.ErrorContainer,
    onErrorContainer = DarkAppColors.OnErrorContainer,
    background = DarkAppColors.Background,
    onBackground = DarkAppColors.OnBackground,
    surface = DarkAppColors.Surface,
    onSurface = DarkAppColors.OnSurface,
    surfaceVariant = DarkAppColors.SurfaceVariant,
    onSurfaceVariant = DarkAppColors.OnSurfaceVariant,
    outline = DarkAppColors.Outline,
    outlineVariant = DarkAppColors.OutlineVariant,
)

/**
 * App theme composable
 *
 * @param darkTheme Whether to use dark theme
 * @param content Content to render
 */
@Composable
fun AppTheme(
    darkTheme: Boolean = isSystemInDarkTheme(),
    content: @Composable () -> Unit
) {{
    val colors = if (darkTheme) DarkColors else LightColors

    MaterialTheme(
        colorScheme = colors,
        typography = Typography,
        shapes = Shapes,
        content = content
    )
}}
"#,
        package = package_name,
    );

    let relative_path = "presentation/ui/theme/Theme.kt";
    match write_generated_file(output_dir, base_package, relative_path, &content, generator.skip_existing)? {
        crate::kotlin::generators::WriteOutcome::Written(p) | crate::kotlin::generators::WriteOutcome::Skipped(p) => Ok(p),
    }
}

/// Generate Type.kt with typography definitions
fn generate_type(
    generator: &MobileGenerator,
    output_dir: &Path,
) -> Result<std::path::PathBuf> {
    let base_package = &generator.package_name;
    let package_name = format!("{}.presentation.ui.theme", base_package);

    let content = format!(
        r#"package {package}

import androidx.compose.material3.Typography
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.sp

/**
 * Typography for the application
 *
 * Generated from Backbone schema
 */
val Typography = Typography(
    displayLarge = TextStyle(
        fontFamily = FontFamily.Default,
        fontWeight = FontWeight.W400,
        fontSize = 57.sp,
        lineHeight = 64.sp,
        letterSpacing = (-0.25).sp,
    ),
    displayMedium = TextStyle(
        fontFamily = FontFamily.Default,
        fontWeight = FontWeight.W400,
        fontSize = 45.sp,
        lineHeight = 52.sp,
        letterSpacing = 0.sp,
    ),
    displaySmall = TextStyle(
        fontFamily = FontFamily.Default,
        fontWeight = FontWeight.W400,
        fontSize = 36.sp,
        lineHeight = 44.sp,
        letterSpacing = 0.sp,
    ),
    headlineLarge = TextStyle(
        fontFamily = FontFamily.Default,
        fontWeight = FontWeight.W400,
        fontSize = 32.sp,
        lineHeight = 40.sp,
        letterSpacing = 0.sp,
    ),
    headlineMedium = TextStyle(
        fontFamily = FontFamily.Default,
        fontWeight = FontWeight.W400,
        fontSize = 28.sp,
        lineHeight = 36.sp,
        letterSpacing = 0.sp,
    ),
    headlineSmall = TextStyle(
        fontFamily = FontFamily.Default,
        fontWeight = FontWeight.W400,
        fontSize = 24.sp,
        lineHeight = 32.sp,
        letterSpacing = 0.sp,
    ),
    titleLarge = TextStyle(
        fontFamily = FontFamily.Default,
        fontWeight = FontWeight.W400,
        fontSize = 22.sp,
        lineHeight = 28.sp,
        letterSpacing = 0.sp,
    ),
    titleMedium = TextStyle(
        fontFamily = FontFamily.Default,
        fontWeight = FontWeight.Medium,
        fontSize = 16.sp,
        lineHeight = 24.sp,
        letterSpacing = 0.15.sp,
    ),
    titleSmall = TextStyle(
        fontFamily = FontFamily.Default,
        fontWeight = FontWeight.Medium,
        fontSize = 14.sp,
        lineHeight = 20.sp,
        letterSpacing = 0.1.sp,
    ),
    bodyLarge = TextStyle(
        fontFamily = FontFamily.Default,
        fontWeight = FontWeight.W400,
        fontSize = 16.sp,
        lineHeight = 24.sp,
        letterSpacing = 0.5.sp,
    ),
    bodyMedium = TextStyle(
        fontFamily = FontFamily.Default,
        fontWeight = FontWeight.W400,
        fontSize = 14.sp,
        lineHeight = 20.sp,
        letterSpacing = 0.25.sp,
    ),
    bodySmall = TextStyle(
        fontFamily = FontFamily.Default,
        fontWeight = FontWeight.W400,
        fontSize = 12.sp,
        lineHeight = 16.sp,
        letterSpacing = 0.4.sp,
    ),
    labelLarge = TextStyle(
        fontFamily = FontFamily.Default,
        fontWeight = FontWeight.W500,
        fontSize = 14.sp,
        lineHeight = 20.sp,
        letterSpacing = 0.1.sp,
    ),
    labelMedium = TextStyle(
        fontFamily = FontFamily.Default,
        fontWeight = FontWeight.Medium,
        fontSize = 12.sp,
        lineHeight = 16.sp,
        letterSpacing = 0.5.sp,
    ),
    labelSmall = TextStyle(
        fontFamily = FontFamily.Default,
        fontWeight = FontWeight.Medium,
        fontSize = 11.sp,
        lineHeight = 16.sp,
        letterSpacing = 0.5.sp,
    ),
)
"#,
        package = package_name,
    );

    let relative_path = "presentation/ui/theme/Type.kt";
    match write_generated_file(output_dir, base_package, relative_path, &content, generator.skip_existing)? {
        crate::kotlin::generators::WriteOutcome::Written(p) | crate::kotlin::generators::WriteOutcome::Skipped(p) => Ok(p),
    }
}

/// Generate Shape.kt with shape definitions
fn generate_shape(
    generator: &MobileGenerator,
    output_dir: &Path,
) -> Result<std::path::PathBuf> {
    let base_package = &generator.package_name;
    let package_name = format!("{}.presentation.ui.theme", base_package);

    let content = format!(
        r#"package {package}

import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Shapes
import androidx.compose.ui.unit.dp

/**
 * Shape definitions for the application
 *
 * Generated from Backbone schema
 */
val Shapes = Shapes(
    extraSmall = RoundedCornerShape(4.dp),
    small = RoundedCornerShape(8.dp),
    medium = RoundedCornerShape(12.dp),
    large = RoundedCornerShape(16.dp),
    extraLarge = RoundedCornerShape(28.dp),
)
"#,
        package = package_name,
    );

    let relative_path = "presentation/ui/theme/Shape.kt";
    match write_generated_file(output_dir, base_package, relative_path, &content, generator.skip_existing)? {
        crate::kotlin::generators::WriteOutcome::Written(p) | crate::kotlin::generators::WriteOutcome::Skipped(p) => Ok(p),
    }
}
