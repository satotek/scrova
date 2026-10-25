buildscript {
    repositories {
        google()
        mavenCentral()
        gradlePluginPortal()
    }
    dependencies {
        // AGP 9.4's built-in Kotlin pins KGP 2.4.21. The Compose compiler must
        // match the Kotlin compiler, so raise KGP to the catalog's Kotlin
        // version. buildscript blocks cannot read version-catalog accessors,
        // hence the literal here.
        classpath("org.jetbrains.kotlin:kotlin-gradle-plugin:2.4.21")
    }
}

plugins {
    alias(libs.plugins.android.application) apply false
    alias(libs.plugins.kotlin.compose) apply false
}
