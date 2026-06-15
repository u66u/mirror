Findings

  1. Folder selection is not enforced by the durable upload queue.
     app/src/main/kotlin/app/mirror/vault/backup/BackupRepository.kt:106 only flips selected, but app/src/main/kotlin/app/mirror/vault/backup/data/BackupDao.kt:114 picks pending media from all present rows without joining backup_folders for selected = 1
     AND available = 1. Counts have the same problem at app/src/main/kotlin/app/mirror/vault/backup/data/BackupDao.kt:203.
     Result: a folder can be deselected yet already-queued media from it can still upload. This is the biggest real correctness flaw I saw.

  2. Backup state is not scoped to the authenticated server/device.
     Logout only cancels workers at app/src/main/kotlin/app/mirror/vault/backup/BackupRepository.kt:53. The Room schema has no serverUrl, account id, or device-token scope, and app/src/main/kotlin/app/mirror/vault/backup/data/BackupDao.kt:83 preserves
     previous verified state for identical local media.
     Result: disconnect from server A, connect to server B, and photos verified on A may be treated as already backed up on B. Either namespace backup rows by remote identity or reset upload/verified state when the credential changes.

  3. The worker tries to drain the entire queue in one background job.
     app/src/main/kotlin/app/mirror/vault/backup/BackupRunner.kt:33 loops until no pending media, while app/src/main/kotlin/app/mirror/vault/backup/work/BackupWork.kt:23 is a plain CoroutineWorker with no foreground mode/progress notification/chunk cap.
     For real photo libraries, this is fragile under Android background limits. Prefer bounded batches plus rescheduling, or foreground WorkManager for long uploads.

  4. “Disconnect” depends on successful remote revoke.
     app/src/main/kotlin/app/mirror/vault/auth/AuthRepository.kt:45 clears local credentials only after api.revoke(). If the server is offline, the user cannot locally sign out cleanly. That is poor UX and not great security posture. Local clear should
     be possible, with remote revoke best-effort or retried.

  5. Cleartext is globally enabled.
     app/src/main/AndroidManifest.xml:20 sets usesCleartextTraffic="true". The app does have solid endpoint validation in app/src/main/kotlin/app/mirror/vault/network/ServerEndpoint.kt:28, so this is not reckless, but the platform-level guardrail is
     gone for all future HTTP code. I’d document this loudly or isolate it with a stricter network-security approach if feasible.

  Slop / Weirdness

  The root tests/ and tests-instrumentation/ source sets in app/build.gradle.kts:44 are non-standard for Android. It works, but most Android developers expect app/src/test and app/src/androidTest.

  .editorconfig:3 disables ktlint function naming globally to accommodate Compose. Detekt partially replaces it, but it is broader than necessary.

  org.bouncycastle:bcprov-jdk18on at app/build.gradle.kts:107 is heavy for only Blake3Digest in app/src/main/kotlin/app/mirror/vault/backup/UploadCoordinator.kt:63. Release shrinking gets the APK to 4.2 MB, but debug is 40 MB and release R8 took 2m17s.
  Acceptable, not elegant.

  Good Decisions

  Manual DI via Application is fine for this size. Room as a durable queue, WorkManager for background scheduling, MediaStore as the source of truth, Keystore-backed token storage, strict lint/detekt/ktlint, warnings-as-errors, release shrinking, schema
  export, and real instrumentation tests are all good calls.

Where It Is Not Proper Yet
  The biggest issues are not style. They are correctness boundaries:

  - queued media is not filtered by current folder selection before upload;
  - backup state is not scoped to the current server/account;
  - long backup work is not bounded or foregrounded;
  - local logout depends on successful remote revoke.

  Those are real design issues to fix before trusting backup behavior.

  Redundancy / Repetition
  Some mild redundancy:

  - room-ktx plus room-runtime is probably redundant because room-ktx already depends on runtime.
  - Ktor API code manually builds/parses JSON; typed @Serializable DTOs would reduce boilerplate and mistakes.
  - backup_rules.xml / data_extraction_rules.xml exist even though allowBackup=false; harmless but a bit redundant.
  - Source layout uses top-level tests/ and tests-instrumentation/, which works but is non-standard versus app/src/test and app/src/androidTest.
