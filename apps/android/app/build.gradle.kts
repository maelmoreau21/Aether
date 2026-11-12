import java.util.Properties

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
}

/**
 * Release signing, never committed: `keystore.properties` next to this file
 * (storeFile, storePassword, keyAlias, keyPassword) or the AETHER_KEYSTORE,
 * AETHER_KEYSTORE_PASSWORD, AETHER_KEY_ALIAS and AETHER_KEY_PASSWORD variables.
 * Without them the release build is produced unsigned.
 */
val signing = Properties().apply {
    rootProject.file("keystore.properties").takeIf { it.isFile }?.inputStream()?.use(::load)
    System.getenv("AETHER_KEYSTORE")?.let { setProperty("storeFile", it) }
    System.getenv("AETHER_KEYSTORE_PASSWORD")?.let { setProperty("storePassword", it) }
    System.getenv("AETHER_KEY_ALIAS")?.let { setProperty("keyAlias", it) }
    System.getenv("AETHER_KEY_PASSWORD")?.let { setProperty("keyPassword", it) }
}

android {
    namespace = "app.aether"
    compileSdk = 35

    defaultConfig {
        applicationId = "app.aether"
        minSdk = 26
        targetSdk = 35
        // Store builds bump these: ./gradlew -PversionCode=2 -PversionName=0.2.0
        versionCode = (project.findProperty("versionCode") as String?)?.toInt() ?: 1
        versionName = (project.findProperty("versionName") as String?) ?: "0.1.0"
        // The Rust core ships for real phones and the x86_64 emulator.
        ndk { abiFilters += listOf("arm64-v8a", "x86_64") }
    }
    signingConfigs {
        if (signing.getProperty("storeFile") != null) {
            create("release") {
                storeFile = file(signing.getProperty("storeFile"))
                storePassword = signing.getProperty("storePassword")
                keyAlias = signing.getProperty("keyAlias")
                keyPassword = signing.getProperty("keyPassword")
            }
        }
    }
    buildTypes {
        release {
            signingConfig = signingConfigs.findByName("release")
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
        }
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions { jvmTarget = "17" }
    buildFeatures { compose = true }
    // Kotlin bindings generated from the Rust library by scripts/build.sh.
    sourceSets["main"].java.srcDir("build/generated/uniffi")
}

dependencies {
    val composeBom = platform("androidx.compose:compose-bom:2024.12.01")
    implementation(composeBom)
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.material:material-icons-core")
    implementation("androidx.activity:activity-compose:1.9.3")
    implementation("androidx.lifecycle:lifecycle-viewmodel-compose:2.8.7")
    implementation("androidx.lifecycle:lifecycle-runtime-compose:2.8.7")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.9.0")
    // Hardware-decoded HLS playback (the Core serves packet-copied fMP4 HLS).
    implementation("androidx.media3:media3-exoplayer:1.5.1")
    implementation("androidx.media3:media3-exoplayer-hls:1.5.1")
    implementation("androidx.media3:media3-ui:1.5.1")
    // UniFFI's Kotlin bindings call the Rust library through JNA.
    implementation("net.java.dev.jna:jna:5.15.0@aar")
    // Posters: memory and disk cache, downsampling; versioned URLs are cached for good.
    implementation("io.coil-kt:coil-compose:2.7.0")
    // Daily device-token refresh, even when the app is not opened.
    implementation("androidx.work:work-runtime-ktx:2.9.1")
    // Chromecast (needs Google Play services; hidden where they are missing, e.g. Fire TV).
    implementation("com.google.android.gms:play-services-cast-framework:21.5.0")
    implementation("androidx.mediarouter:mediarouter:1.7.0")
    // The Cast route picker is an AppCompat dialog: it needs an AppCompat theme.
    implementation("androidx.appcompat:appcompat:1.7.0")
    // Single sign-on in the phone's own browser (a tab over the application).
    implementation("androidx.browser:browser:1.8.0")
    // The web interface: bridge scripts and messages limited to the server's own pages.
    implementation("androidx.webkit:webkit:1.12.1")
}
