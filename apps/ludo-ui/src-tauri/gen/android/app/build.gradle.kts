import java.util.Properties
import org.jetbrains.kotlin.gradle.dsl.JvmTarget

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("rust")
}

val tauriProperties = Properties().apply {
    val propFile = file("tauri.properties")
    if (propFile.exists()) {
        propFile.inputStream().use { load(it) }
    }
}

// Signature de la version release (mise à jour depuis l'app : Android n'accepte une nouvelle version
// que signée par la MÊME clé). CI : variables LUDO_ANDROID_* (secrets du dépôt) ; poste :
// ~/.config/ludotheque/android/signature.properties (scripts/android-cle.sh). Sans clé : APK non signé.
val signatureLocale = Properties().apply {
    val f = File(System.getProperty("user.home"), ".config/ludotheque/android/signature.properties")
    if (f.exists()) f.inputStream().use { load(it) }
}
fun signature(env: String, key: String): String? =
    System.getenv(env)?.takeIf { it.isNotEmpty() } ?: signatureLocale.getProperty(key)?.takeIf { it.isNotEmpty() }
val cleRelease = signature("LUDO_ANDROID_KEYSTORE", "storeFile")

android {
    compileSdk = 37
    signingConfigs {
        if (cleRelease != null) {
            create("release") {
                storeFile = file(cleRelease)
                storePassword = signature("LUDO_ANDROID_KEYSTORE_PASSWORD", "storePassword")
                keyAlias = signature("LUDO_ANDROID_KEY_ALIAS", "keyAlias") ?: "ludotheque"
                keyPassword = signature("LUDO_ANDROID_KEY_PASSWORD", "keyPassword") ?: storePassword
            }
        }
    }
    namespace = "fr.srvdreamer.ludotheque"
    defaultConfig {
        manifestPlaceholders["usesCleartextTraffic"] = "false"
        applicationId = "fr.srvdreamer.ludotheque"
        minSdk = 24
        targetSdk = 37
        versionCode = tauriProperties.getProperty("tauri.android.versionCode", "1").toInt()
        versionName = tauriProperties.getProperty("tauri.android.versionName", "1.0")
    }
    buildTypes {
        getByName("debug") {
            applicationIdSuffix = ".debug"
            manifestPlaceholders["usesCleartextTraffic"] = "true"
            isDebuggable = true
            isJniDebuggable = true
            isMinifyEnabled = false
            packaging {
                jniLibs.keepDebugSymbols.add("*/arm64-v8a/*.so")
                jniLibs.keepDebugSymbols.add("*/armeabi-v7a/*.so")
                jniLibs.keepDebugSymbols.add("*/x86/*.so")
                jniLibs.keepDebugSymbols.add("*/x86_64/*.so")
            }
        }
        getByName("release") {
            if (cleRelease != null) signingConfig = signingConfigs.getByName("release")
            optimization {
               enable = true
            }
            proguardFiles(
                *fileTree(".") {
                  include("**/*.pro")
                  exclude("build/**")
                }.files.toTypedArray()
            )
        }
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_1_8
        targetCompatibility = JavaVersion.VERSION_1_8
    }
    buildFeatures {
        buildConfig = true
    }
}

kotlin {
    compilerOptions {
        jvmTarget = JvmTarget.JVM_1_8
    }
}

rust {
    rootDirRel = "../../../"
}

dependencies {
    implementation("androidx.webkit:webkit:1.14.0")
    implementation("androidx.appcompat:appcompat:1.7.1")
    implementation("androidx.activity:activity-ktx:1.10.1")
    implementation("com.google.android.material:material:1.12.0")
    implementation("androidx.lifecycle:lifecycle-process:2.10.0")
    testImplementation("junit:junit:4.13.2")
    androidTestImplementation("androidx.test.ext:junit:1.1.4")
    androidTestImplementation("androidx.test.espresso:espresso-core:3.5.0")
}

apply(from = file("tauri.build.gradle.kts"))
