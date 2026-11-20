<script setup lang="ts">
import { MegaphoneIcon } from '@modrinth/assets'
import { Button, useVIntl } from '@modrinth/ui'

import { requestAnnouncements, useAnnouncements } from '@/composables/use-announcements'
import { announcementMessages as messages } from '@/helpers/announcement-messages'

const { snapshot, active } = useAnnouncements()
const { formatMessage } = useVIntl()
</script>

<template>
	<Button
		v-if="snapshot.visible_count > 0"
		type="quiet"
		:class="{ 'opacity-60 grayscale': active.length === 0 }"
		@click="requestAnnouncements"
	>
		<MegaphoneIcon aria-hidden="true" />
		{{ formatMessage(messages.title) }}
	</Button>
</template>
