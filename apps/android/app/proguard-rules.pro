# JNA binds native methods and reads Structure fields reflectively, so R8 must
# not rename or remove these classes in release builds.
-keep class com.sun.jna.** { *; }
-keepclassmembers class * extends com.sun.jna.Structure { *; }
-keepclassmembers class * implements com.sun.jna.Callback { *; }
-keepclassmembers class * extends com.sun.jna.Library { *; }
-keepclasseswithmembernames class * {
    native <methods>;
}
-dontwarn java.awt.**
-dontwarn javax.swing.**

# UniFFI-generated classes are reached from JNA and from Kotlin by name.
-keep class uniffi.** { *; }
-keepclassmembers class uniffi.** { *; }

-keepattributes *Annotation*, InnerClasses, Signature
