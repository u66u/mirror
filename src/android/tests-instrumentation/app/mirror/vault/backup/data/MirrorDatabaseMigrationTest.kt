package app.mirror.vault.backup.data

import androidx.room.testing.MigrationTestHelper
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class MirrorDatabaseMigrationTest {
    @get:Rule
    val helper =
        MigrationTestHelper(
            InstrumentationRegistry.getInstrumentation(),
            MirrorDatabase::class.java,
        )

    @Test
    fun migration_1_2_preserves_settings_and_adds_remote_fence() {
        helper.createDatabase(DATABASE_NAME, 1).apply {
            execSQL(
                """
                INSERT INTO backup_settings(id, wifiOnly, folderSelectionInitialized)
                VALUES(1, 0, 1)
                """.trimIndent(),
            )
            close()
        }

        helper
            .runMigrationsAndValidate(
                DATABASE_NAME,
                2,
                true,
                MirrorDatabase.MIGRATION_1_2,
            ).use { database ->
                database
                    .query(
                        """
                        SELECT wifiOnly, folderSelectionInitialized, remoteScope, remoteGeneration
                        FROM backup_settings
                        WHERE id = 1
                        """.trimIndent(),
                    ).use { cursor ->
                        check(cursor.moveToFirst())
                        assertEquals(0, cursor.getInt(0))
                        assertEquals(1, cursor.getInt(1))
                        assertEquals(null, cursor.getString(2))
                        assertEquals(0, cursor.getLong(3))
                    }
            }
    }

    companion object {
        private const val DATABASE_NAME = "migration-test"
    }
}
