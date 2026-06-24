# Codex Judgment
High: Page、PageHeader、Slot 字段都是 pub，但一致性校验是私有的 [page_op.rs (line 233)](/Users/zxe6b885/Home/austrini/stor_bptree/src/bptree/page_op.rs:233)。上层可以直接改 free_start/free_end/slots/data，导致 free_space() 下溢、encode() panic 或写出坏页。建议先把字段收紧为私有，暴露受控方法。

High: PageId = 0 被用作 None 哨兵 [utils.rs (line 3)](/Users/zxe6b885/Home/austrini/stor_bptree/src/bptree/utils.rs:3)，但 new_leaf/new_internal 没有禁止真实页使用 0 [page_op.rs (line 121)](/Users/zxe6b885/Home/austrini/stor_bptree/src/bptree/page_op.rs:121)。如果 allocator 从 0 开始，父子指针会和空指针混淆。建议用 NonZeroU64，或强制页分配从 1 开始并校验。

High: 目前 page 只存 opaque bytes [page_op.rs (line 150)](/Users/zxe6b885/Home/austrini/stor_bptree/src/bptree/page_op.rs:150)，LeafCell/InternalCell 没有序列化格式、key 提取、比较接口 [types.rs (line 118)](/Users/zxe6b885/Home/austrini/stor_bptree/src/bptree/types.rs:118)。上层 B+Tree 要做查找、插入定位、split separator key，必须先定义 cell wire format 或 codec trait。

Medium: internal page 的不变量不足。new_internal 默认没有 left_most_child_page_id [page_op.rs (line 125)](/Users/zxe6b885/Home/austrini/stor_bptree/src/bptree/page_op.rs:125)，validate() 只禁止 leaf 有 left-most child，没有保证 internal 的 child 数量等于 key_count + 1。这会让路由查找无法可靠实现。

Medium: slot 校验只检查单个 slot 的范围 [utils.rs (line 46)](/Users/zxe6b885/Home/austrini/stor_bptree/src/bptree/utils.rs:46)，没有检查 slot 之间是否重叠、是否覆盖了声明的 cell data 区域。decode() 会接受部分结构损坏但 checksum 正确的页 [page_op.rs (line 50)](/Users/zxe6b885/Home/austrini/stor_bptree/src/bptree/page_op.rs:50)。

Medium: mutation API 只有 insert_cell_bytes，没有 delete/update/clear/compact/split 辅助。可以勉强在上层重建页面完成 split，但如果先把字段私有化，就需要补一个安全的重建或分裂接口。

# TODO
1. new_leaf/new_internal: 增加 page_id 合法性校验。
2. 读取 Page 增加 LeafCell/InternalCell 的序列化格式、key 提起、比较接口。提供上层 BP Tree 查找、插入定位、split separator key 使用的 cell wire format 或 codec trait。
3. internal page 的 child 数量等于 key_cound + 1，增加 internal page 路由查找的可靠性。
4. slot 合法性校验，检查 slot 之间是否重叠，是否覆盖声明的 cell data 区域。
5. cell decode/encode 的合法性校验，确保操作的 page 结构正确。

# Design
1. 
2. LeafCell/InternalCell: decode from page
3. slot validation