# JNA and the UniFFI bindings are reached by reflection from native code.
-keep class com.sun.jna.** { *; }
-keep class * extends com.sun.jna.** { *; }
-keep class uniffi.** { *; }
-dontwarn java.awt.**
