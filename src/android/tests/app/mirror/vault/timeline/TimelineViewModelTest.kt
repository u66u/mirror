package app.mirror.vault.timeline

import app.mirror.vault.auth.DeviceCredential
import app.mirror.vault.network.AssetDerivative
import app.mirror.vault.network.AssetTimelineItem
import app.mirror.vault.network.AssetTimelinePage
import app.mirror.vault.network.TimelineApi
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.delay
import kotlinx.coroutines.test.StandardTestDispatcher
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.resetMain
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.test.setMain
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Before
import org.junit.Test

@OptIn(ExperimentalCoroutinesApi::class)
class TimelineViewModelTest {
    private val dispatcher = StandardTestDispatcher()

    @Before
    fun setMain() {
        Dispatchers.setMain(dispatcher)
    }

    @After
    fun resetMain() {
        Dispatchers.resetMain()
    }

    @Test
    fun appendsPagesAndIgnoresStaleCredentialResponse() =
        runTest(dispatcher.scheduler) {
            val oldCredential = credential("https://old.example")
            val newCredential = credential("https://new.example")
            val api =
                FakeTimelineApi(
                    pages =
                        mapOf(
                            "https://old.example" to listOf(page("old-asset", null, delayMillis = 10)),
                            "https://new.example" to
                                listOf(
                                    page("new-asset-1", "cursor-2"),
                                    page("new-asset-2", null),
                                ),
                        ),
                )
            val viewModel =
                TimelineViewModel(
                    repository = TimelineRepository(api),
                    ioDispatcher = dispatcher,
                )

            viewModel.setCredential(oldCredential)
            viewModel.setCredential(newCredential)
            advanceUntilIdle()
            viewModel.loadNext()
            advanceUntilIdle()

            assertEquals(
                listOf("new-asset-1", "new-asset-2"),
                viewModel.state.value.items
                    .map { it.assetId },
            )
            assertNull(viewModel.state.value.nextCursor)
        }

    @Test
    fun invalidCredentialUrlDoesNotEscapeRenderUrlHelper() =
        runTest(dispatcher.scheduler) {
            val viewModel =
                TimelineViewModel(
                    repository =
                        TimelineRepository(
                            FakeTimelineApi(mapOf("not a url" to listOf(page("asset-1", null)))),
                        ),
                    ioDispatcher = dispatcher,
                )

            viewModel.setCredential(credential("not a url"))
            advanceUntilIdle()

            assertNull(
                viewModel.derivativeUrl(
                    asset = item("asset-1"),
                    kind = TimelineDerivativeKind.THUMBNAIL,
                ),
            )
        }
}

private class FakeTimelineApi(
    private val pages: Map<String, List<FakePage>>,
) : TimelineApi {
    private val cursors = mutableMapOf<String, Int>()

    override suspend fun listAssets(
        credential: DeviceCredential,
        cursor: String?,
        limit: Int,
    ): AssetTimelinePage {
        val index = if (cursor == null) 0 else cursors.getValue(credential.serverUrl)
        val page = pages.getValue(credential.serverUrl)[index]
        cursors[credential.serverUrl] = index + 1
        delay(page.delayMillis)
        return AssetTimelinePage(
            items = listOf(item(page.assetId)),
            nextCursor = page.nextCursor,
        )
    }
}

private data class FakePage(
    val assetId: String,
    val nextCursor: String?,
    val delayMillis: Long,
)

private fun page(
    assetId: String,
    nextCursor: String?,
    delayMillis: Long = 0,
): FakePage =
    FakePage(
        assetId = assetId,
        nextCursor = nextCursor,
        delayMillis = delayMillis,
    )

private fun credential(serverUrl: String): DeviceCredential =
    DeviceCredential(
        serverUrl = serverUrl,
        deviceTokenId = "device",
        token = "secret",
    )

private fun item(assetId: String): AssetTimelineItem =
    AssetTimelineItem(
        assetId = assetId,
        createdAt = "2026-06-17T00:00:00Z",
        favoriteAt = null,
        originalBlake3 = "hash",
        mediaType = "image/jpeg",
        sizeBytes = 10,
        originalFilename = "$assetId.jpg",
        thumbnail = AssetDerivative("webp", 256, 256),
        preview = AssetDerivative("webp", 1280, 720),
    )
