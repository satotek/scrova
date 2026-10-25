package dev.satotek.scrova.ui

import org.junit.Assert.*
import org.junit.Test

class PresentationTest {
    @Test fun historyDayLabelsUseLocalDates() {
        val today = java.time.LocalDate.of(2026, 10, 25)
        assertEquals("Today", historyDayLabel(today, today))
        assertEquals("Yesterday", historyDayLabel(today.minusDays(1), today))
        assertTrue(historyDayLabel(today.minusYears(1), today).contains("2025"))
    }
    @Test fun markdownPreservesWrappedBulletsAndLinks() {
        val blocks = markdownBlocks("# Privacy\n\nLast updated: today\n\n- **Music.** First line\n  continued [link](https://example.com).\n- Another item\n\n## Contact\n\nWrite here.")
        assertEquals(6, blocks.size)
        assertEquals(1, blocks.first().heading)
        assertEquals("Last updated: today", blocks[1].text)
        assertTrue(blocks[2].bullet)
        assertEquals("**Music.** First line continued [link](https://example.com).", blocks[2].text)
        assertEquals(2, blocks[4].heading)
        assertEquals("Write here.", blocks.last().text)
    }

    @Test fun lastfmLinksEncodeEachSegment() {
        assertEquals("https://www.last.fm/music/AC%2FDC/_/A%20%26%20B%3F", lastfmTrackUrl("AC/DC", "A & B?"))
        assertEquals("https://www.last.fm/user/name%20%2F%20%2B", lastfmProfileUrl("name / +"))
    }
}
