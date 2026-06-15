package app.mirror.vault.backup.data

import android.content.Context
import androidx.room.Database
import androidx.room.Room
import androidx.room.RoomDatabase
import androidx.room.migration.Migration
import androidx.sqlite.db.SupportSQLiteDatabase

@Database(
    entities = [
        BackupFolderEntity::class,
        BackupMediaEntity::class,
        BackupSettingsEntity::class,
    ],
    version = 2,
    exportSchema = true,
)
abstract class MirrorDatabase : RoomDatabase() {
    abstract fun backupDao(): BackupDao

    companion object {
        fun create(context: Context): MirrorDatabase =
            Room
                .databaseBuilder(
                    context.applicationContext,
                    MirrorDatabase::class.java,
                    "mirror.db",
                ).addMigrations(MIGRATION_1_2)
                .build()

        val MIGRATION_1_2: Migration =
            object : Migration(1, 2) {
                override fun migrate(db: SupportSQLiteDatabase) {
                    db.execSQL(
                        "ALTER TABLE backup_media " +
                            "ADD COLUMN remoteGeneration INTEGER NOT NULL DEFAULT 0",
                    )
                    db.execSQL(
                        "ALTER TABLE backup_settings ADD COLUMN remoteScope TEXT",
                    )
                    db.execSQL(
                        "ALTER TABLE backup_settings " +
                            "ADD COLUMN remoteGeneration INTEGER NOT NULL DEFAULT 0",
                    )
                    db.execSQL(
                        "DROP INDEX IF EXISTS index_backup_media_state_present_modifiedAtSeconds",
                    )
                    db.execSQL(
                        "CREATE INDEX IF NOT EXISTS " +
                            "index_backup_media_remoteGeneration_state_present_modifiedAtSeconds " +
                            "ON backup_media(remoteGeneration, state, present, modifiedAtSeconds)",
                    )
                }
            }
    }
}
