package com.mirror.mirror

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.GridItemSpan
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.AutoAwesome
import androidx.compose.material.icons.filled.CloudDone
import androidx.compose.material.icons.filled.Collections
import androidx.compose.material.icons.filled.DarkMode
import androidx.compose.material.icons.filled.Favorite
import androidx.compose.material.icons.filled.GridView
import androidx.compose.material.icons.filled.Image
import androidx.compose.material.icons.filled.Info
import androidx.compose.material.icons.filled.LightMode
import androidx.compose.material.icons.filled.Map
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material.icons.filled.Movie
import androidx.compose.material.icons.filled.People
import androidx.compose.material.icons.filled.Person
import androidx.compose.material.icons.filled.PhotoAlbum
import androidx.compose.material.icons.filled.Search
import androidx.compose.material.icons.filled.Tune
import androidx.compose.material.icons.outlined.AutoAwesome
import androidx.compose.material.icons.outlined.Collections
import androidx.compose.material.icons.outlined.Image
import androidx.compose.material.icons.outlined.Map
import androidx.compose.material.icons.outlined.PhotoAlbum
import androidx.compose.material.icons.outlined.Search
import androidx.compose.material3.AssistChip
import androidx.compose.material3.AssistChipDefaults
import androidx.compose.material3.Badge
import androidx.compose.material3.BadgedBox
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.CenterAlignedTopAppBar
import androidx.compose.material3.ElevatedAssistChip
import androidx.compose.material3.ElevatedCard
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilledIconButton
import androidx.compose.material3.FilterChip
import androidx.compose.material3.FloatingActionButton
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.NavigationRail
import androidx.compose.material3.NavigationRailItem
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SearchBar
import androidx.compose.material3.SearchBarDefaults
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.scale
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import com.mirror.mirror.ui.theme.MirrorTheme

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        setContent {
            var darkTheme by rememberSaveable { mutableStateOf(false) }
            MirrorTheme(darkTheme = darkTheme) {
                MirrorHome(
                    darkTheme = darkTheme,
                    onToggleTheme = { darkTheme = !darkTheme },
                )
            }
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class, ExperimentalFoundationApi::class)
@Composable
fun MirrorHome(
    darkTheme: Boolean,
    onToggleTheme: () -> Unit,
    modifier: Modifier = Modifier,
) {
    var selectedNav by rememberSaveable { mutableIntStateOf(0) }
    var selectedFilter by rememberSaveable { mutableIntStateOf(0) }
    var selectedView by rememberSaveable { mutableIntStateOf(0) }
    var query by rememberSaveable { mutableStateOf("") }
    var searchExpanded by rememberSaveable { mutableStateOf(false) }
    var selectedPhoto by rememberSaveable { mutableStateOf<MemoryPhoto?>(null) }
    val navItems = remember { mirrorNavItems }

    BoxWithConstraints(modifier = modifier.fillMaxSize()) {
        val wideLayout = maxWidth >= 700.dp
        Row(Modifier.fillMaxSize()) {
            if (wideLayout) {
                MirrorNavigationRail(
                    selectedIndex = selectedNav,
                    navItems = navItems,
                    onSelect = { selectedNav = it },
                )
            }

            Scaffold(
                modifier = Modifier.weight(1f),
                containerColor = MaterialTheme.colorScheme.background,
                contentWindowInsets = WindowInsets.safeDrawing,
                topBar = {
                    MirrorTopBar(
                        darkTheme = darkTheme,
                        onToggleTheme = onToggleTheme,
                    )
                },
                bottomBar = {
                    if (!wideLayout) {
                        MirrorNavigationBar(
                            selectedIndex = selectedNav,
                            navItems = navItems,
                            onSelect = { selectedNav = it },
                        )
                    }
                },
                floatingActionButton = {
                    FloatingActionButton(
                        onClick = { selectedPhoto = memoryPhotos.first() },
                        containerColor = MaterialTheme.colorScheme.primaryContainer,
                        contentColor = MaterialTheme.colorScheme.onPrimaryContainer,
                    ) {
                        Icon(Icons.Filled.Add, contentDescription = "Create")
                    }
                },
            ) { innerPadding ->
                LazyVerticalGrid(
                    columns = GridCells.Adaptive(128.dp),
                    modifier = Modifier
                        .fillMaxSize()
                        .padding(innerPadding),
                    contentPadding = PaddingValues(
                        start = if (wideLayout) 32.dp else 16.dp,
                        end = if (wideLayout) 32.dp else 16.dp,
                        top = 12.dp,
                        bottom = 112.dp,
                    ),
                    horizontalArrangement = Arrangement.spacedBy(6.dp),
                    verticalArrangement = Arrangement.spacedBy(6.dp),
                ) {
                    item(span = { GridItemSpan(maxLineSpan) }) {
                        MirrorSearchBar(
                            query = query,
                            expanded = searchExpanded,
                            onQueryChange = { query = it },
                            onExpandedChange = { searchExpanded = it },
                        )
                    }
                    item(span = { GridItemSpan(maxLineSpan) }) {
                        MoodFilters(
                            selectedFilter = selectedFilter,
                            onSelect = { selectedFilter = it },
                        )
                    }
                    item(span = { GridItemSpan(maxLineSpan) }) {
                        HighlightsHeader(
                            selectedView = selectedView,
                            onSelectedViewChange = { selectedView = it },
                        )
                    }
                    item(span = { GridItemSpan(maxLineSpan) }) {
                        TodayStoryCard(onClick = { selectedPhoto = memoryPhotos[3] })
                    }
                    item(span = { GridItemSpan(maxLineSpan) }) {
                        AlbumRail()
                    }
                    item(span = { GridItemSpan(maxLineSpan) }) {
                        DateHeader("Today", "142 backed up")
                    }
                    items(memoryPhotos.take(9), key = { it.id }) { photo ->
                        PhotoTile(
                            photo = photo,
                            onClick = { selectedPhoto = photo },
                        )
                    }
                    item(span = { GridItemSpan(maxLineSpan) }) {
                        DateHeader("Yesterday", "68 items")
                    }
                    items(memoryPhotos.drop(9), key = { it.id }) { photo ->
                        PhotoTile(
                            photo = photo,
                            onClick = { selectedPhoto = photo },
                        )
                    }
                }
            }
        }
    }

    if (selectedPhoto != null) {
        PhotoDetailsSheet(
            photo = selectedPhoto!!,
            onDismiss = { selectedPhoto = null },
        )
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun MirrorTopBar(
    darkTheme: Boolean,
    onToggleTheme: () -> Unit,
) {
    CenterAlignedTopAppBar(
        title = {
            Row(
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                Box(
                    modifier = Modifier
                        .size(30.dp)
                        .clip(CircleShape)
                        .background(
                            Brush.linearGradient(
                                listOf(
                                    MaterialTheme.colorScheme.primary,
                                    MaterialTheme.colorScheme.tertiary,
                                ),
                            ),
                        ),
                    contentAlignment = Alignment.Center,
                ) {
                    Icon(
                        Icons.Filled.AutoAwesome,
                        contentDescription = null,
                        tint = MaterialTheme.colorScheme.onPrimary,
                        modifier = Modifier.size(16.dp),
                    )
                }
                Text("Mirror", maxLines = 1)
            }
        },
        navigationIcon = {
            IconButton(onClick = {}) {
                Icon(Icons.Filled.Person, contentDescription = "Account")
            }
        },
        actions = {
            IconButton(onClick = onToggleTheme) {
                Icon(
                    imageVector = if (darkTheme) Icons.Filled.LightMode else Icons.Filled.DarkMode,
                    contentDescription = "Toggle theme",
                )
            }
            IconButton(onClick = {}) {
                Icon(Icons.Filled.MoreVert, contentDescription = "More")
            }
        },
        colors = TopAppBarDefaults.topAppBarColors(
            containerColor = MaterialTheme.colorScheme.background.copy(alpha = 0.92f),
        ),
    )
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun MirrorSearchBar(
    query: String,
    expanded: Boolean,
    onQueryChange: (String) -> Unit,
    onExpandedChange: (Boolean) -> Unit,
) {
    SearchBar(
        inputField = {
            SearchBarDefaults.InputField(
                query = query,
                onQueryChange = onQueryChange,
                onSearch = { onExpandedChange(false) },
                expanded = expanded,
                onExpandedChange = onExpandedChange,
                placeholder = { Text("Search photos, people, places") },
                leadingIcon = { Icon(Icons.Filled.Search, contentDescription = null) },
                trailingIcon = { Icon(Icons.Filled.Tune, contentDescription = "Tune") },
            )
        },
        expanded = expanded,
        onExpandedChange = onExpandedChange,
        modifier = Modifier
            .fillMaxWidth()
            .padding(vertical = 4.dp),
        colors = SearchBarDefaults.colors(
            containerColor = MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.72f),
        ),
    ) {
        Column(
            modifier = Modifier.padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            SearchSuggestion("Singapore skyline", Icons.Outlined.Map)
            SearchSuggestion("Videos from this week", Icons.Filled.Movie)
            SearchSuggestion("Favorites with Mara", Icons.Filled.Favorite)
        }
    }
}

@Composable
private fun SearchSuggestion(label: String, icon: ImageVector) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .clip(MaterialTheme.shapes.medium)
            .clickable {}
            .padding(12.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Icon(icon, contentDescription = null, tint = MaterialTheme.colorScheme.primary)
        Text(label, style = MaterialTheme.typography.bodyLarge)
    }
}

@Composable
private fun MoodFilters(
    selectedFilter: Int,
    onSelect: (Int) -> Unit,
) {
    val filters = listOf("All", "People", "Trips", "Videos", "Favorites")
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .horizontalScroll(rememberScrollState()),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        filters.forEachIndexed { index, label ->
            FilterChip(
                selected = selectedFilter == index,
                onClick = { onSelect(index) },
                label = { Text(label) },
                leadingIcon = if (selectedFilter == index) {
                    { Icon(Icons.Filled.AutoAwesome, contentDescription = null, modifier = Modifier.size(18.dp)) }
                } else {
                    null
                },
            )
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun HighlightsHeader(
    selectedView: Int,
    onSelectedViewChange: (Int) -> Unit,
) {
    Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
        Row(
            modifier = Modifier.fillMaxWidth(),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.SpaceBetween,
        ) {
            Column {
                Text("Library", style = MaterialTheme.typography.displaySmall)
                Text(
                    "Your best moments, arranged by time",
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    style = MaterialTheme.typography.bodyMedium,
                )
            }
            AssistChip(
                onClick = {},
                label = { Text("Synced") },
                leadingIcon = {
                    Icon(Icons.Filled.CloudDone, contentDescription = null, modifier = Modifier.size(18.dp))
                },
                colors = AssistChipDefaults.assistChipColors(
                    leadingIconContentColor = MaterialTheme.colorScheme.primary,
                ),
            )
        }

        SingleChoiceSegmentedButtonRow {
            listOf("Timeline", "Albums", "Map").forEachIndexed { index, label ->
                SegmentedButton(
                    selected = selectedView == index,
                    onClick = { onSelectedViewChange(index) },
                    shape = SegmentedButtonDefaults.itemShape(index = index, count = 3),
                    icon = {},
                    label = { Text(label, maxLines = 1) },
                )
            }
        }
    }
}

@Composable
private fun TodayStoryCard(onClick: () -> Unit) {
    ElevatedCard(
        onClick = onClick,
        modifier = Modifier
            .fillMaxWidth()
            .heightIn(min = 156.dp),
        colors = CardDefaults.elevatedCardColors(
            containerColor = MaterialTheme.colorScheme.primaryContainer,
            contentColor = MaterialTheme.colorScheme.onPrimaryContainer,
        ),
        elevation = CardDefaults.elevatedCardElevation(defaultElevation = 2.dp),
    ) {
        Box(
            modifier = Modifier
                .fillMaxWidth()
                .height(174.dp)
                .background(
                    Brush.linearGradient(
                        listOf(
                            MaterialTheme.colorScheme.primary,
                            MaterialTheme.colorScheme.tertiary,
                            MaterialTheme.colorScheme.secondaryContainer,
                        ),
                    ),
                ),
        ) {
            Column(
                modifier = Modifier
                    .align(Alignment.BottomStart)
                    .padding(18.dp),
                verticalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                ElevatedAssistChip(
                    onClick = {},
                    label = { Text("AI memory") },
                    leadingIcon = {
                        Icon(Icons.Outlined.AutoAwesome, contentDescription = null, modifier = Modifier.size(18.dp))
                    },
                )
                Text(
                    "Colorful week",
                    style = MaterialTheme.typography.headlineMedium,
                    color = Color.White,
                )
                Text(
                    "A vibrant collection from city nights, food, and friends.",
                    style = MaterialTheme.typography.bodyMedium,
                    color = Color.White.copy(alpha = 0.86f),
                )
            }
        }
    }
}

@Composable
private fun AlbumRail() {
    Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text("Albums", style = MaterialTheme.typography.titleLarge)
            Text(
                "View all",
                color = MaterialTheme.colorScheme.primary,
                style = MaterialTheme.typography.labelLarge,
            )
        }
        LazyRow(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
            items(albumItems.size) { index ->
                AlbumCard(albumItems[index])
            }
        }
    }
}

@Composable
private fun AlbumCard(album: AlbumItem) {
    Card(
        modifier = Modifier.width(156.dp),
        colors = CardDefaults.cardColors(
            containerColor = MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.66f),
        ),
        border = BorderStroke(1.dp, MaterialTheme.colorScheme.outlineVariant),
    ) {
        Box(
            modifier = Modifier
                .fillMaxWidth()
                .height(108.dp)
                .background(Brush.linearGradient(album.colors)),
        ) {
            Icon(
                album.icon,
                contentDescription = null,
                tint = Color.White,
                modifier = Modifier
                    .align(Alignment.TopEnd)
                    .padding(12.dp),
            )
        }
        Column(
            modifier = Modifier.padding(12.dp),
            verticalArrangement = Arrangement.spacedBy(2.dp),
        ) {
            Text(album.title, style = MaterialTheme.typography.titleMedium, maxLines = 1)
            Text(
                album.count,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                style = MaterialTheme.typography.bodyMedium,
            )
        }
    }
}

@Composable
private fun DateHeader(title: String, count: String) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .padding(top = 16.dp, bottom = 4.dp),
        verticalAlignment = Alignment.Bottom,
        horizontalArrangement = Arrangement.SpaceBetween,
    ) {
        Column {
            Text(title, style = MaterialTheme.typography.titleLarge)
            Text(
                count,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                style = MaterialTheme.typography.bodyMedium,
            )
        }
        IconButton(onClick = {}) {
            Icon(Icons.Filled.GridView, contentDescription = "Change grid")
        }
    }
}

@Composable
private fun PhotoTile(
    photo: MemoryPhoto,
    onClick: () -> Unit,
) {
    val scale by animateFloatAsState(targetValue = if (photo.favorite) 1.0f else 0.94f, label = "favoriteScale")
    Box(
        modifier = Modifier
            .aspectRatio(photo.ratio)
            .clip(MaterialTheme.shapes.medium)
            .background(Brush.linearGradient(photo.colors))
            .clickable(onClick = onClick),
    ) {
        if (photo.favorite) {
            FilledIconButton(
                onClick = {},
                modifier = Modifier
                    .align(Alignment.TopEnd)
                    .padding(6.dp)
                    .size(34.dp)
                    .scale(scale),
            ) {
                Icon(
                    Icons.Filled.Favorite,
                    contentDescription = "Favorite",
                    modifier = Modifier.size(18.dp),
                )
            }
        }
        Column(
            modifier = Modifier
                .align(Alignment.BottomStart)
                .fillMaxWidth()
                .background(
                    Brush.verticalGradient(
                        listOf(Color.Transparent, Color.Black.copy(alpha = 0.56f)),
                    ),
                )
                .padding(10.dp),
        ) {
            Text(
                photo.title,
                color = Color.White,
                style = MaterialTheme.typography.labelLarge,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            Text(
                photo.place,
                color = Color.White.copy(alpha = 0.82f),
                style = MaterialTheme.typography.labelSmall,
                maxLines = 1,
            )
        }
    }
}

@Composable
private fun MirrorNavigationBar(
    selectedIndex: Int,
    navItems: List<MirrorNavItem>,
    onSelect: (Int) -> Unit,
) {
    NavigationBar(
        containerColor = MaterialTheme.colorScheme.surface,
        tonalElevation = 3.dp,
    ) {
        navItems.forEachIndexed { index, item ->
            NavigationBarItem(
                selected = selectedIndex == index,
                onClick = { onSelect(index) },
                icon = {
                    BadgedNavIcon(
                        item = item,
                        selected = selectedIndex == index,
                    )
                },
                label = { Text(item.label) },
            )
        }
    }
}

@Composable
private fun MirrorNavigationRail(
    selectedIndex: Int,
    navItems: List<MirrorNavItem>,
    onSelect: (Int) -> Unit,
) {
    NavigationRail(
        modifier = Modifier
            .fillMaxHeight()
            .windowInsetsPadding(WindowInsets.safeDrawing),
        containerColor = MaterialTheme.colorScheme.surface,
        header = {
            FloatingActionButton(
                onClick = {},
                modifier = Modifier.padding(vertical = 16.dp),
                containerColor = MaterialTheme.colorScheme.primaryContainer,
                contentColor = MaterialTheme.colorScheme.onPrimaryContainer,
            ) {
                Icon(Icons.Filled.Add, contentDescription = "Create")
            }
        },
    ) {
        navItems.forEachIndexed { index, item ->
            NavigationRailItem(
                selected = selectedIndex == index,
                onClick = { onSelect(index) },
                icon = {
                    BadgedNavIcon(
                        item = item,
                        selected = selectedIndex == index,
                    )
                },
                label = { Text(item.label) },
            )
        }
    }
}

@Composable
private fun BadgedNavIcon(
    item: MirrorNavItem,
    selected: Boolean,
) {
    BadgedBox(
        badge = {
            AnimatedVisibility(item.badgeCount > 0) {
                Badge { Text(item.badgeCount.toString()) }
            }
        },
    ) {
        Icon(
            imageVector = if (selected) item.selectedIcon else item.icon,
            contentDescription = item.label,
        )
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun PhotoDetailsSheet(
    photo: MemoryPhoto,
    onDismiss: () -> Unit,
) {
    ModalBottomSheet(
        onDismissRequest = onDismiss,
        sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = false),
    ) {
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = 24.dp)
                .padding(bottom = 36.dp),
            verticalArrangement = Arrangement.spacedBy(18.dp),
        ) {
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Column {
                    Text(photo.title, style = MaterialTheme.typography.headlineMedium)
                    Text(
                        photo.place,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        style = MaterialTheme.typography.bodyLarge,
                    )
                }
                FilledIconButton(onClick = {}) {
                    Icon(Icons.Filled.Favorite, contentDescription = "Favorite")
                }
            }
            Box(
                modifier = Modifier
                    .fillMaxWidth()
                    .height(210.dp)
                    .clip(MaterialTheme.shapes.extraLarge)
                    .background(Brush.linearGradient(photo.colors)),
            )
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                AssistChip(onClick = {}, label = { Text("4K") })
                AssistChip(onClick = {}, label = { Text("Backed up") })
                AssistChip(onClick = {}, label = { Text("Shared") })
            }
            HorizontalDivider()
            Row(
                horizontalArrangement = Arrangement.spacedBy(12.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Icon(Icons.Filled.Info, contentDescription = null, tint = MaterialTheme.colorScheme.primary)
                Text(
                    "Frontend-only detail view. Metadata and sharing hooks can connect later.",
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
    }
}

private data class MirrorNavItem(
    val label: String,
    val icon: ImageVector,
    val selectedIcon: ImageVector,
    val badgeCount: Int = 0,
)

private data class AlbumItem(
    val title: String,
    val count: String,
    val icon: ImageVector,
    val colors: List<Color>,
)

private data class MemoryPhoto(
    val id: Int,
    val title: String,
    val place: String,
    val ratio: Float,
    val favorite: Boolean,
    val colors: List<Color>,
)

private val mirrorNavItems = listOf(
    MirrorNavItem("Photos", Icons.Outlined.Image, Icons.Filled.Image, 2),
    MirrorNavItem("Collections", Icons.Outlined.Collections, Icons.Filled.Collections),
    MirrorNavItem("Search", Icons.Outlined.Search, Icons.Filled.Search),
    MirrorNavItem("Places", Icons.Outlined.Map, Icons.Filled.Map),
)

private val albumItems = listOf(
    AlbumItem("People", "18 faces", Icons.Filled.People, listOf(Color(0xFF7C4DFF), Color(0xFFFF5C8D))),
    AlbumItem("Trips", "312 items", Icons.Filled.Map, listOf(Color(0xFF00A6FF), Color(0xFF9C6BFF))),
    AlbumItem("Videos", "42 clips", Icons.Filled.Movie, listOf(Color(0xFFFF7043), Color(0xFFFFC400))),
    AlbumItem("Archive", "120 items", Icons.Filled.PhotoAlbum, listOf(Color(0xFF26A69A), Color(0xFF66BB6A))),
)

private val memoryPhotos = listOf(
    MemoryPhoto(1, "Morning glass", "Home", 1.0f, true, listOf(Color(0xFF42A5F5), Color(0xFF7E57C2))),
    MemoryPhoto(2, "Market flowers", "Lisbon", 0.78f, false, listOf(Color(0xFFFF8A80), Color(0xFFFFD180))),
    MemoryPhoto(3, "Mara laughing", "Studio", 1.0f, true, listOf(Color(0xFFE040FB), Color(0xFF7C4DFF))),
    MemoryPhoto(4, "Glass towers", "Singapore", 1.35f, false, listOf(Color(0xFF00BCD4), Color(0xFF536DFE))),
    MemoryPhoto(5, "Dinner light", "Brooklyn", 0.86f, true, listOf(Color(0xFFFF7043), Color(0xFFFFC107))),
    MemoryPhoto(6, "Train window", "Tokyo", 1.0f, false, listOf(Color(0xFF26C6DA), Color(0xFF00E676))),
    MemoryPhoto(7, "Museum corner", "Madrid", 0.78f, false, listOf(Color(0xFFB388FF), Color(0xFFFF80AB))),
    MemoryPhoto(8, "Blue hour", "Reykjavik", 1.35f, true, listOf(Color(0xFF536DFE), Color(0xFF64B5F6))),
    MemoryPhoto(9, "Coffee bar", "Portland", 1.0f, false, listOf(Color(0xFFFFA726), Color(0xFF8D6E63))),
    MemoryPhoto(10, "Canal walk", "Amsterdam", 1.0f, false, listOf(Color(0xFF4DB6AC), Color(0xFF9575CD))),
    MemoryPhoto(11, "Neon rain", "Seoul", 0.78f, true, listOf(Color(0xFFEC407A), Color(0xFF5C6BC0))),
    MemoryPhoto(12, "Kitchen light", "Home", 1.35f, false, listOf(Color(0xFFFFD54F), Color(0xFFFF8A65))),
    MemoryPhoto(13, "Trail edge", "Boulder", 1.0f, false, listOf(Color(0xFF66BB6A), Color(0xFF29B6F6))),
    MemoryPhoto(14, "Night mural", "Austin", 0.78f, true, listOf(Color(0xFFAB47BC), Color(0xFFFF5252))),
    MemoryPhoto(15, "Sunday table", "Paris", 1.0f, false, listOf(Color(0xFFFF80AB), Color(0xFFFFD180))),
)

@Preview(showBackground = true, widthDp = 412, heightDp = 900)
@Composable
private fun MirrorHomeLightPreview() {
    MirrorTheme(darkTheme = false) {
        Surface {
            MirrorHome(darkTheme = false, onToggleTheme = {})
        }
    }
}

@Preview(showBackground = true, widthDp = 412, heightDp = 900)
@Composable
private fun MirrorHomeDarkPreview() {
    MirrorTheme(darkTheme = true) {
        Surface {
            MirrorHome(darkTheme = true, onToggleTheme = {})
        }
    }
}

@Preview(showBackground = true, widthDp = 900, heightDp = 700)
@Composable
private fun MirrorHomeWidePreview() {
    MirrorTheme(darkTheme = false) {
        Surface {
            MirrorHome(darkTheme = false, onToggleTheme = {})
        }
    }
}
