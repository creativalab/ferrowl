plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.compose)
    alias(libs.plugins.kotlin.serialization)
    alias(libs.plugins.ksp)
    alias(libs.plugins.hilt)
    // Reads app/google-services.json (Firebase project "ferrowl"): needed for push.
    alias(libs.plugins.google.services)
}

android {
    namespace = "com.ferrowl.app"
    // Latest AndroidX libraries need the Android 37 SDK platform to compile.
    compileSdk = 37

    defaultConfig {
        applicationId = "com.ferrowl.app"
        minSdk = 26
        targetSdk = 37
        versionCode = 7
        versionName = "0.4.12"
        // The ABIs the Rust engine is built for (see ffi/build-android.sh).
        ndk { abiFilters += listOf("arm64-v8a", "armeabi-v7a", "x86_64") }
    }

    buildTypes {
        release {
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
        }
    }

    buildFeatures {
        compose = true
        buildConfig = true
    }

    lint {
        // CI treats every warning as an error, so the code stays clean.
        warningsAsErrors = true
        abortOnError = true
        // Version freshness is handled by dependency updates, not by Lint.
        disable += setOf("AndroidGradlePluginVersion", "GradleDependency", "NewerVersionAvailable")
    }
}

dependencies {
    implementation(project(":core"))

    implementation(libs.androidx.core.ktx)
    implementation(libs.androidx.activity.compose)
    implementation(libs.androidx.lifecycle.viewmodel.compose)
    implementation(libs.androidx.lifecycle.runtime.compose)
    implementation(libs.androidx.lifecycle.process)
    implementation(libs.androidx.navigation.compose)
    implementation(libs.androidx.datastore.preferences)
    implementation(libs.androidx.work.runtime.ktx)
    implementation(libs.androidx.browser)
    implementation(libs.play.services.auth)
    implementation(libs.billing.ktx)
    implementation(libs.kotlinx.serialization.json)

    implementation(platform(libs.compose.bom))
    implementation(libs.compose.ui)
    implementation(libs.compose.material3)
    implementation(libs.compose.material.icons)
    implementation(libs.compose.ui.tooling.preview)
    debugImplementation(libs.compose.ui.tooling)

    implementation(libs.hilt.android)
    ksp(libs.hilt.compiler)
    implementation(libs.hilt.navigation.compose)

    // Push notifications (Firebase Cloud Messaging).
    implementation(platform(libs.firebase.bom))
    implementation(libs.firebase.messaging)
    implementation(libs.kotlinx.coroutines.play.services)

    testImplementation(libs.junit)
}

// ---------------------------------------------------------------- strings
// UI texts come from the same files as the desktop app (core/locales/*.toml,
// flat `key = "value"` lines) and are turned into Android string resources
// at build time, so a translation is written once for both.

abstract class GenerateStrings : DefaultTask() {
    @get:InputDirectory
    @get:PathSensitive(PathSensitivity.RELATIVE)
    abstract val locales: DirectoryProperty

    @get:OutputDirectory
    abstract val outputDir: DirectoryProperty

    @TaskAction
    fun generate() {
        val out = outputDir.get().asFile
        out.deleteRecursively()
        val line = Regex("""^([a-z0-9_]+) = "(.*)"$""")
        locales.get().asFile.listFiles { f -> f.extension == "toml" }!!.sortedBy { it.name }.forEach { file ->
            val code = file.nameWithoutExtension
            // English is the default (values/); the others go to values-<code>/.
            val dir = out.resolve(if (code == "en") "values" else "values-$code")
            dir.mkdirs()
            val xml = StringBuilder("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<!-- Generated from core/locales/$code.toml: edit that file. -->\n<resources>\n")
            file.readLines().mapNotNull { line.matchEntire(it.trim()) }.forEach { m ->
                val (key, value) = m.destructured
                xml.append("    <string name=\"$key\" formatted=\"false\">${escape(value)}</string>\n")
            }
            xml.append("</resources>\n")
            dir.resolve("strings_core.xml").writeText(xml.toString())
        }
    }

    private fun escape(s: String): String =
        s.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")
            .replace("\\\"", "\"").replace("\"", "\\\"").replace("'", "\\'")
            .replace("@", "\\@").replace("?", "\\?")
}

val generateStrings by tasks.registering(GenerateStrings::class) {
    locales.set(rootProject.layout.projectDirectory.dir("../core/locales"))
    outputDir.set(layout.buildDirectory.dir("generated/res/locales"))
}

androidComponents {
    onVariants { variant ->
        variant.sources.res?.addGeneratedSourceDirectory(generateStrings, GenerateStrings::outputDir)
    }
}
