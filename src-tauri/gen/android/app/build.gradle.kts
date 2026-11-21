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

// Release signing: the upload key and its passwords live outside the repository, in
// %USERPROFILE%\.paginaut\android-signing.properties (or the file PAGINAUT_ANDROID_SIGNING names):
// storeFile, storePassword, keyAlias, keyPassword. Without it, release builds come out unsigned (tools/android.sh
// refuses to make them).
val signingProperties = Properties().apply {
    val path = System.getenv("PAGINAUT_ANDROID_SIGNING")
        ?: "${System.getProperty("user.home")}/.paginaut/android-signing.properties"
    val f = File(path)
    if (f.exists()) f.inputStream().use { load(it) }
}

android {
    compileSdk = 37
    if (signingProperties.getProperty("storeFile") != null) {
        signingConfigs {
            create("release") {
                storeFile = File(signingProperties.getProperty("storeFile"))
                storePassword = signingProperties.getProperty("storePassword")
                keyAlias = signingProperties.getProperty("keyAlias")
                keyPassword = signingProperties.getProperty("keyPassword")
            }
        }
    }
    namespace = "app.paginaut.reader"
    defaultConfig {
        manifestPlaceholders["usesCleartextTraffic"] = "false"
        applicationId = "app.paginaut.reader"
        minSdk = 24
        targetSdk = 37
        versionCode = tauriProperties.getProperty("tauri.android.versionCode", "1").toInt()
        versionName = tauriProperties.getProperty("tauri.android.versionName", "1.0")
    }
    buildTypes {
        getByName("debug") {
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
            signingConfigs.findByName("release")?.let { signingConfig = it }
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
    implementation("androidx.webkit:webkit:1.17.1")
    implementation("androidx.appcompat:appcompat:1.8.0")
    implementation("androidx.activity:activity-ktx:1.13.0")
    implementation("com.google.android.material:material:1.14.0")
    implementation("androidx.lifecycle:lifecycle-process:2.11.0")
    testImplementation("junit:junit:4.13.2")
    androidTestImplementation("androidx.test.ext:junit:1.1.4")
    androidTestImplementation("androidx.test.espresso:espresso-core:3.5.0")
}

// Tauri's Jackson (2.15) refers to java.lang.BootstrapMethodError, which Android 7 doesn't have: the
// app stopped at start there. 2.13 reads and writes the same JSON for Tauri.
configurations.all {
    resolutionStrategy.eachDependency {
        if (requested.group.startsWith("com.fasterxml.jackson")) useVersion("2.13.5")
    }
}

apply(from = file("tauri.build.gradle.kts"))

// ---------------------------------------------------------------- what the app asks Android for
//
// Every build stops if the merged manifest (the app's own plus every library's) asks for a permission
// Paginaut doesn't use, opens a component to other apps, or shares more than the copies of opened
// documents and a problem report. A library update can add any of these without a word; this says so and stops. MainActivity
// checks the permissions again when the app starts, and won't run with extra ones.
val allowedPermissions = setOf(
    "android.permission.MANAGE_EXTERNAL_STORAGE",
    "android.permission.READ_EXTERNAL_STORAGE",
    "app.paginaut.reader.DYNAMIC_RECEIVER_NOT_EXPORTED_PERMISSION",
)
val filePathsXml = file("src/main/res/xml/file_paths.xml")

// Written out in the task (no calls back into this script: Gradle's configuration cache can't keep those).
tasks.configureEach {
    if (!(name.startsWith("process") && name.endsWith("MainManifest"))) return@configureEach
    // Debug builds load the page from the computer building them.
    val debug = name.contains("Debug")
    val allowed = allowedPermissions + if (debug) setOf("android.permission.INTERNET") else emptySet()
    val pathsXml = filePathsXml
    doLast {
        val ns = "http://schemas.android.com/apk/res/android"
        val problems = mutableListOf<String>()
        for (manifest in outputs.files.asFileTree.matching { include("**/AndroidManifest.xml") }.files) {
            val doc = javax.xml.parsers.DocumentBuilderFactory.newInstance().apply { isNamespaceAware = true }.newDocumentBuilder().parse(manifest)
            for (tag in listOf("uses-permission", "uses-permission-sdk-23")) {
                val list = doc.getElementsByTagName(tag)
                for (i in 0 until list.length) {
                    val name = (list.item(i) as org.w3c.dom.Element).getAttributeNS(ns, "name")
                    if (name !in allowed) problems.add("asks for a permission Paginaut doesn't use: $name")
                }
            }
            // Reachable from other apps: the main screen (launcher, "Open with", Share) and AndroidX's
            // profile receiver, which only the system's shell may call (DUMP).
            for (tag in listOf("activity", "activity-alias", "service", "receiver", "provider")) {
                val list = doc.getElementsByTagName(tag)
                for (i in 0 until list.length) {
                    val e = list.item(i) as org.w3c.dom.Element
                    if (e.getAttributeNS(ns, "exported") != "true") continue
                    val name = e.getAttributeNS(ns, "name")
                    val ok = (tag == "activity" && name == "app.paginaut.reader.MainActivity") ||
                        (tag == "receiver" && e.getAttributeNS(ns, "permission") == "android.permission.DUMP")
                    if (!ok) problems.add("opens a $tag to other apps: $name")
                }
            }
            val app = doc.getElementsByTagName("application").item(0) as org.w3c.dom.Element
            if (!debug && app.getAttributeNS(ns, "debuggable") == "true") problems.add("is debuggable")
        }
        // What Share may hand out: the cache's copies of opened documents, and a problem report the user shares
        // (PaginautPlugin.shareReport, cache/report/): nothing else.
        val paths = javax.xml.parsers.DocumentBuilderFactory.newInstance().newDocumentBuilder().parse(pathsXml)
        val entries = paths.documentElement.childNodes.let { n -> (0 until n.length).map { n.item(it) }.filterIsInstance<org.w3c.dom.Element>() }
        val shared = entries.map { it.tagName + ":" + it.getAttribute("path") }.toSet()
        if (entries.size != 2 || shared != setOf("cache-path:docs/", "cache-path:report/")) {
            problems.add("shares more than the copies of opened documents and a problem report (res/xml/file_paths.xml)")
        }
        if (problems.isNotEmpty()) {
            throw GradleException(
                "Paginaut stopped this build. The app " + problems.joinToString(";\n  ", "\n  ", "\n") +
                    "Remove it, or if it's really needed, add it to app/build.gradle.kts and MainActivity's list.",
            )
        }
    }
}
