#import <Foundation/Foundation.h>
#import <UIKit/UIKit.h>

NS_ASSUME_NONNULL_BEGIN

@interface PREPPreparedProseViewerFabricEventHarness : NSObject
- (instancetype)initWithSource:(NSString *)source
                     configJSON:(NSString *)configJSON
                      themeJSON:(NSString *)themeJSON
                      surfaceID:(int64_t)surfaceID
                    componentTag:(int64_t)componentTag
                     leaseHandle:(uint64_t)leaseHandle
                           width:(CGFloat)width
                           scale:(CGFloat)scale;
@property(nonatomic, readonly) NSArray<NSDictionary<NSString *, id> *> *events;
@property(nonatomic, readonly) UIView *drawingView;
- (void)setTableLogicalOffset:(CGFloat)offset sourceIdentity:(NSString *)sourceIdentity;
- (void)setHostHidden:(BOOL)hidden;
- (void)setLayoutWidth:(CGFloat)width;
- (void)beginPendingReplacementWithWidth:(CGFloat)width;
- (void)prepareReplacementAndInstall;
- (void)expireLease;
- (void)replaceLease:(uint64_t)leaseHandle;
- (void)recycle;
@end

NS_ASSUME_NONNULL_END
