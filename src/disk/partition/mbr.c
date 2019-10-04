#include <disk/partition/partition.h>

void mbr_etop(const mbr_entry_t entry, mbr_partition_t *partition)
{
	if(!entry || !partition)
		return;
	bzero(partition, sizeof(mbr_partition_t));
	partition->attrs = entry[0];
	memcpy(((void *) &partition->chs_addr) + 1, entry + 1, 3);
	partition->partition_type = entry[4];
	memcpy(((void *) &partition->chs_addr_last) + 1, entry + 5, 3);
	partition->start_lba = *(uint32_t *) (entry + 8);
	partition->sectors = *(uint32_t *) (entry + 12);
}

void mbr_ptoe(mbr_partition_t *partition, mbr_entry_t entry)
{
	if(!partition || !entry)
		return;
	entry[0] = partition->attrs;
	memcpy(entry + 1, ((void *) &partition->chs_addr) + 1, 3);
	entry[4] = partition->partition_type;
	memcpy(entry + 5, ((void *) &partition->chs_addr_last) + 1, 3);
	*(uint32_t *) (entry + 8) = partition->start_lba;
	*(uint32_t *) (entry + 12) = partition->sectors;
}

void mbr_read(ata_device_t *dev, size_t lba, mbr_partition_t *partitions)
{
	char buff[ATA_SECTOR_SIZE];
	mbr_t mbr;
	size_t i;

	if(!dev || !partitions)
		return;
	ata_read(dev, lba, buff, 1);
	memcpy(&mbr, buff + MBR_PARTITION_TABLE_OFFSET, sizeof(mbr_t));
	for(i = 0; i < MBR_ENTRIES_COUNT; ++i)
		mbr_etop(mbr.entries[i], partitions + i);
}

void mbr_write(ata_device_t *dev, size_t lba, mbr_t *mbr)
{
	char buff[ATA_SECTOR_SIZE];

	if(!dev || !mbr)
		return;
	ata_read(dev, lba, buff, 1);
	memcpy(buff + MBR_PARTITION_TABLE_OFFSET, mbr, sizeof(mbr_t));
	ata_write(dev, lba, buff, 1);
}