import java.util.Properties

plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.compose)
}

// Last.fm credentials stay out of git, like Config/Secrets.xcconfig on Apple.
// A secret shipped in an app cannot be hidden; this only keeps it out of the
// repository. Missing values build fine and disable sign-in.
val localProperties = Properties().apply {
    val file = rootProject.file("local.properties")
    if (file.isFile) file.inputStream().use(::load)
}
fun lastfmProperty(name: String): String = localProperties.getProperty("lastfm.$name", "")

android {
    namespace = "dev.satotek.scrova"

    compileSdk {
        version = release(37) {
            minorApiLevel = 1
        }
    }

    defaultConfig {
        applicationId = "dev.satotek.scrova.android"
        minSdk = 26
        targetSdk = 37
        versionCode = 1
        versionName = "0.1.0"

        // scripts/build-android.sh produces these two ABIs; packaging others
        // would advertise support without the matching Rust library.
        ndk {
            abiFilters += listOf("arm64-v8a", "x86_64")
        }

        buildConfigField("String", "LASTFM_API_KEY", "\"${lastfmProperty("apiKey")}\"")
        buildConfigField("String", "LASTFM_SHARED_SECRET", "\"${lastfmProperty("sharedSecret")}\"")
    }

    buildTypes {
        release {
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro",
            )
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    buildFeatures {
        compose = true
        buildConfig = true
    }

    sourceSets {
        getByName("main") {
            kotlin.directories += "src/main/generated/kotlin"
        }
    }

    packaging {
        resources {
            excludes += "/META-INF/{AL2.0,LGPL2.1}"
        }
    }
}

// The generated bindings are produced outside Gradle by scripts/build-android.sh.
// Fail before Kotlin compilation so a missing generation step is obvious.
val generatedBindings = layout.projectDirectory.dir("src/main/generated/kotlin")
val checkUniffiBindings = tasks.register("checkUniffiBindings") {
    doLast {
        val root = generatedBindings.asFile
        val hasKotlin = root.isDirectory &&
            root.walkTopDown().any { it.isFile && it.extension == "kt" }
        if (!hasKotlin) {
            throw GradleException(
                "UniFFI Kotlin bindings are missing from ${root.path}. " +
                    "Run scripts/build-android.sh from the repository root, then rebuild.",
            )
        }
    }
}
tasks.named("preBuild") {
    dependsOn(checkUniffiBindings)
}

dependencies {
    implementation(libs.androidx.datastore.core)
    testImplementation(libs.junit)
    implementation(platform(libs.androidx.compose.bom))
    implementation(libs.androidx.compose.ui)
    implementation(libs.androidx.compose.material3)
    implementation(libs.androidx.compose.material.icons.core)
    implementation(libs.androidx.compose.ui.tooling.preview)
    implementation(libs.androidx.activity.compose)
    implementation(libs.androidx.lifecycle.runtime.ktx)
    implementation(libs.androidx.lifecycle.viewmodel.compose)
    implementation(libs.androidx.browser)
    implementation(libs.androidx.work.runtime.ktx)
    implementation(libs.androidx.datastore.preferences)
    implementation(libs.kotlinx.coroutines.android)

    // UniFFI's generated Kotlin and JNA need the AAR variant, not the desktop JAR.
    implementation("net.java.dev.jna:jna:${libs.versions.jna.get()}@aar")

    debugImplementation(libs.androidx.compose.ui.tooling)
}
