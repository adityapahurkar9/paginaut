/*
 * libarchive configuration for Paginaut: it only reads RAR comic books (CBR) from memory, on
 * Android (NDK clang) and Windows (MSVC). No disk access, no compression libraries, no ACLs.
 * Used through -DPLATFORM_CONFIG_H (see src-tauri/build.rs).
 */
#ifndef PAGINAUT_LIBARCHIVE_CONFIG_H
#define PAGINAUT_LIBARCHIVE_CONFIG_H
#define __LIBARCHIVE_CONFIG_H_INCLUDED 1

#define HAVE_CTYPE_H 1
#define HAVE_ERRNO_H 1
#define HAVE_EILSEQ 1
#define HAVE_INTTYPES_H 1
#define HAVE_LIMITS_H 1
#define HAVE_LOCALE_H 1
#define HAVE_SETLOCALE 1
#define HAVE_STDARG_H 1
#define HAVE_STDINT_H 1
#define HAVE_STDLIB_H 1
#define HAVE_STRING_H 1
#define HAVE_STRCHR 1
#define HAVE_STRDUP 1
#define HAVE_STRERROR 1
#define HAVE_STRRCHR 1
#define HAVE_MEMMOVE 1
#define HAVE_MEMSET 1
#define HAVE_SYS_STAT_H 1
#define HAVE_SYS_TYPES_H 1
#define HAVE_TIME_H 1
#define HAVE_WCHAR_H 1
#define HAVE_WCHAR_T 1
#define HAVE_MBRTOWC 1
#define HAVE_WCRTOMB 1
#define HAVE_WCSCMP 1
#define HAVE_WCSCPY 1
#define HAVE_WCSLEN 1
#define HAVE_WMEMCMP 1
#define HAVE_WMEMCPY 1
#define HAVE_WMEMMOVE 1
#define HAVE_INTMAX_T 1
#define HAVE_UINTMAX_T 1
#define HAVE_LONG_LONG_INT 1
#define HAVE_UNSIGNED_LONG_LONG 1
#define HAVE_UNSIGNED_LONG_LONG_INT 1
#define HAVE_DECL_INT32_MAX 1
#define HAVE_DECL_INT32_MIN 1
#define HAVE_DECL_INT64_MAX 1
#define HAVE_DECL_INT64_MIN 1
#define HAVE_DECL_INTMAX_MAX 1
#define HAVE_DECL_INTMAX_MIN 1
#define HAVE_DECL_SIZE_MAX 1
#define HAVE_DECL_UINT32_MAX 1
#define HAVE_DECL_UINT64_MAX 1
#define HAVE_DECL_UINTMAX_MAX 1

#ifdef _WIN32
#define HAVE_WINDOWS_H 1
#define HAVE_IO_H 1
#define HAVE_DIRECT_H 1
#define HAVE_WINCRYPT_H 1
#define HAVE_BCRYPT_H 1
/* POSIX types MSVC lacks, as libarchive's CMake build sets them for Windows. */
#define gid_t short
#define uid_t short
#define id_t short
#define mode_t unsigned short
#define pid_t int
#if defined(_WIN64)
#define ssize_t int64_t
#else
#define ssize_t long
#endif
#else
#define HAVE_DECL_SSIZE_MAX 1
#define HAVE_UNISTD_H 1
#define HAVE_STRINGS_H 1
#define HAVE_SYS_TIME_H 1
#define HAVE_GMTIME_R 1
#define HAVE_LOCALTIME_R 1
#define HAVE_TIMEGM 1
#define HAVE_PTHREAD_H 1
#define HAVE_FCNTL_H 1
#endif

#endif
