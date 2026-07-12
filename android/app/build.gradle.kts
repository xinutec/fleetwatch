plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.android)
}

android {
    namespace = "org.xinutec.fleetwatch"
    compileSdk = 36
    // Pin to the build-tools the nix SDK provides (AGP would otherwise pick a
    // version that isn't in the read-only SDK).
    buildToolsVersion = "36.0.0"

    defaultConfig {
        applicationId = "org.xinutec.fleetwatch"
        // minSdk 26 (Android 8): the system WebView is Chromium on any such device,
        // so the Angular dashboard renders as it does in Chrome.
        minSdk = 26
        targetSdk = 36
        versionCode = 1
        versionName = "0.1"
    }

    buildTypes {
        // Sideloaded build — no shrinking, signed with the debug key for simplicity.
        release {
            isMinifyEnabled = false
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}

kotlin {
    compilerOptions {
        jvmTarget.set(org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17)
    }
}

dependencies {
    // WebView is part of the framework. core-ktx for the insets/prefs KTX and
    // activity for the modern OnBackPressedDispatcher (predictive back). No
    // Compose, no AppCompat: this app is a single WebView.
    implementation(libs.androidx.core.ktx)
    implementation(libs.androidx.activity)
    // WorkManager: the 30-minute background poll of /api/problems (ProblemsWorker).
    // It's the only reason this app is more than a WebView — it batches the wakeup with
    // the system's, which is what keeps the battery cost of "check every 30 min" at ~0.
    // HTTP + JSON come from the framework (HttpURLConnection, org.json) — no OkHttp, no
    // serialization plugin, in keeping with how small this app is.
    implementation(libs.androidx.work)

    testImplementation(libs.junit)
    // Android's org.json in the unit-test classpath is a stub that throws on every
    // call ("Stub!"). Put the real implementation on the test classpath so the parser
    // is genuinely exercised rather than mocked into always agreeing with us.
    testImplementation(libs.json)
}
