-- all the address lists that we are watching from de DAN layer
create table lists
(
    id                      integer   not NULL primary key AUTOINCREMENT,
    address                 text      not NULL,
    count                   integer   not NULL DEFAULT 0,
);

create unique index uniq_lists_address on lists (address);

-- all the items in the lists, referenciong the substates they are pointing to
create table list_items
(
    id                      integer   not NULL primary key AUTOINCREMENT,
    list_id                 integer   not NULL,
    idx                     integer   not NULL,
    substate_id             integer   not NULL,
    FOREIGN KEY(list_id) REFERENCES lists(id),
    FOREIGN KEY(substate_id) REFERENCES substates(id)
);

-- A list can only have one single item at any specific position
create unique index uniq_list_item on list_items (list_id, idx);

-- Adding a new item must increase the list count by 1
CREATE TRIGGER trg_list_count AFTER INSERT ON list_items
BEGIN
  UPDATE lists 
  SET count = COALESCE(count, 0) + 1 
  WHERE id = NEW.list_id;
END;